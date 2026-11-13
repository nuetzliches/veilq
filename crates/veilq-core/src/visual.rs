//! Images and GIFs (ADR-0023): OCR reads the text, the detectors find
//! personal data in it, and the characters that hold it are filled black.
//!
//! The lines of an image are read in rows, top to bottom, left to right, as
//! one text (two spaces between lines of a row, a line break between rows),
//! so that a label (`Geburtsdatum`) is context for the value beside it.
//! Lines stacked closely in one cell of a table or one paragraph are read
//! first as one line of that text, joined with a space: a row would cut a
//! cell's text into the rows beside it. A
//! finding becomes the boxes of its characters, per line, two pixels wider.
//! Every action is a black box: a pseudonym or a stand-in cannot be written
//! into pixels faithfully. **verify** reads the filled image again; what it
//! still finds is filled too, for up to three rounds (ADR-0022).
//!
//! Faces are found first (ADR-0038), when the policy has them filled: each
//! is a `FACE` finding and filled black before OCR reads the picture. A
//! face in a GIF is looked for in every frame that changed, in the whole
//! frame.
//!
//! A GIF is read frame by frame. A box stays filled in the following frames
//! as long as its pixels do not change there, so a value that stands still
//! is not visible for a frame because OCR missed it once. Unchanged frames
//! are not read again; a frame that changed is read only in bands as wide
//! as the frame around the rows that changed, and whole when those come to
//! more than half of it (ADR-0030).
//!
//! The output is encoded anew, so EXIF, XMP and text chunks are gone; when
//! the input had EXIF (camera, time, often GPS), the report has a `METADATA`
//! finding for it. EXIF orientation is applied first, so boxes and pixels
//! agree.

use std::io::Cursor;

use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::{
    AnimationDecoder, DynamicImage, Frame, ImageDecoder, ImageFormat, ImageReader, Rgba, RgbaImage,
};

use crate::Engine;
use crate::Error;
use crate::finding::{EntityType, Finding, Location};
use crate::ocr::{Line, Ocr, Rect};
use crate::office::Outcome;
use crate::policy::LowConfidence;

const PAD: u32 = 2;

/// Said when a policy asks for `pseudonymize` or `fake` in a picture.
pub const BOXES_ONLY: &str = "in a picture every action is a black box: a pseudonym or a stand-in cannot be written into pixels faithfully";
/// Said when faces were filled: what the detector missed stays.
pub const FACES_FOUND: &str = "faces were found and filled black; the face detector can miss a face (in profile, small, covered, drawn): check the picture";
/// Verify rounds at most: reading a filled picture again moves rows and
/// labels, so a round can find what the one before did not (ADR-0026).
const ROUNDS: usize = 6;

/// The type of a line OCR read badly, filled with `[picture]
/// low_confidence = "box"` (ADR-0045), and its detector.
pub const UNREAD_TEXT: &str = "UNREAD_TEXT";
const UNREAD_DETECTOR: &str = "ocr:low-confidence";

/// Where a finding is, as the report and the messages say it.
fn place(f: &Finding) -> String {
    match &f.location {
        Location::Region {
            frame,
            x,
            y,
            width,
            height,
            ..
        } => at(
            &Rect {
                x: *x,
                y: *y,
                w: *width,
                h: *height,
            },
            *frame,
        ),
        _ => String::new(),
    }
}

/// A region as the messages say it.
fn at(r: &Rect, frame: Option<u64>) -> String {
    let at = format!("{}x{} at {},{}", r.w, r.h, r.x, r.y);
    match frame {
        Some(n) => format!("{at} (frame or page {n})"),
        None => at,
    }
}

/// The review reason for the lines OCR read badly (`weak_lines`), once per
/// file: personal data in them may have been read as something else and
/// not found (#52). A picture is not failed for it; a person looks.
pub fn weak_reason(out: &mut Outcome) {
    let Some(first) = out.weak_lines.first() else {
        return;
    };
    let reason = match out.weak_lines.len() {
        1 => format!(
            "OCR read a line with low confidence at {first} (handwriting, small or blurred text): personal data there may not have been found"
        ),
        n => format!(
            "OCR read {n} lines with low confidence, the first at {first} (handwriting, small or blurred text): personal data there may not have been found"
        ),
    };
    out.review_reasons.push(reason);
    out.weak_lines.clear();
}

fn error(message: impl std::fmt::Display) -> Error {
    Error::Format(message.to_string())
}

/// The text of an image's lines and, per character, its byte range in that
/// text and its box.
struct Page {
    text: String,
    chars: Vec<(usize, usize, Rect)>,
}

fn page(lines: &[Line]) -> Page {
    // Cells first: a table cell wraps its text into lines stacked closely,
    // and a row of the page would cut it (`BHS Bad` / `Homburger` /
    // `Servicegesellschaft` beside one-line cells): the middle line, read
    // between the cells beside it, looks like a surname.
    let mut sorted: Vec<&Line> = lines.iter().collect();
    sorted.sort_by_key(|l| (l.rect.y, l.rect.x));
    let mut stacks: Vec<Vec<&Line>> = Vec::new();
    for l in sorted {
        match stacks.iter_mut().find(|s| below(s[s.len() - 1], l)) {
            Some(s) => s.push(l),
            None => stacks.push(vec![l]),
        }
    }
    let cells: Vec<(Rect, Vec<&Line>)> = cut(&stacks)
        .into_iter()
        .map(|c| (c.iter().skip(1).fold(c[0].rect, |r, l| r.union(&l.rect)), c))
        .collect();
    // Rows: cells whose vertical middles fall into each other's height.
    let mut rows: Vec<Vec<(Rect, Vec<&Line>)>> = Vec::new();
    for cell in cells {
        let best = rows
            .iter()
            .enumerate()
            .map(|(i, r)| (i, overlap_y(&r[0].0, &cell.0)))
            .filter(|(_, o)| *o >= 0.5)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((i, _)) => rows[i].push(cell),
            None => rows.push(vec![cell]),
        }
    }
    let mut text = String::new();
    let mut chars = Vec::new();
    for (i, row) in rows.iter_mut().enumerate() {
        row.sort_by_key(|(r, _)| r.x);
        if i > 0 {
            text.push('\n');
        }
        for (j, (_, cell)) in row.iter().enumerate() {
            if j > 0 {
                text.push_str("  ");
            }
            for (k, l) in cell.iter().enumerate() {
                if k > 0 {
                    text.push(' ');
                }
                let base = text.len();
                text.push_str(&l.text);
                chars.extend(l.chars.iter().map(|(s, e, r)| (base + s, base + e, *r)));
            }
        }
    }
    Page { text, chars }
}

/// Whether `l` continues `above` in the same cell or paragraph: right below
/// it (a gap of at most half a line), in the same type size, and aligned
/// with it on the left, the middle or the right.
fn below(above: &Line, l: &Line) -> bool {
    let (a, b) = (&above.rect, &l.rect);
    let h = a.h.min(b.h);
    // OCR's boxes of lines set closely overlap by a few pixels.
    let close = b.y + h / 4 >= a.bottom() && b.y <= a.bottom() + h / 2;
    let size = 3 * a.h.max(b.h) <= 4 * h;
    close && size && aligned(a, b)
}

/// Whether two boxes stand in one column: aligned on the left, the middle
/// or the right, within a line's height.
fn aligned(a: &Rect, b: &Rect) -> bool {
    let h = a.h.min(b.h);
    a.x.abs_diff(b.x) <= h
        || (a.x + a.right()).abs_diff(b.x + b.right()) <= 2 * h
        || a.right().abs_diff(b.right()) <= h
}

/// The lines in the column of `lines[i]`: above it (`up`) or below, each
/// overlapping it and aligned with it.
fn column(lines: &[Line], i: usize, up: bool) -> impl Iterator<Item = usize> + '_ {
    let l = &lines[i].rect;
    lines.iter().enumerate().filter_map(move |(j, o)| {
        let o = &o.rect;
        let side = if up {
            o.bottom() <= l.y + 2
        } else {
            o.y + 2 >= l.bottom()
        };
        (j != i && side && o.x < l.right() && l.x < o.right() && aligned(l, o)).then_some(j)
    })
}

/// Per line, whether it heads a column: nothing above it in its column,
/// something below.
fn heads(lines: &[Line]) -> Vec<bool> {
    (0..lines.len())
        .map(|i| {
            column(lines, i, true).next().is_none() && column(lines, i, false).next().is_some()
        })
        .collect()
}

/// The cells of `stacks`: a stack cut where the lines beside it show the
/// rows of a table. A table's rule is not read, and the lines of a full
/// cell sit as close to the next row's as to each other; the one-line
/// cells beside (an amount, a date) stand in the middle of their rows, and
/// a stack beside two of them is cut halfway between.
fn cut<'a>(stacks: &[Vec<&'a Line>]) -> Vec<Vec<&'a Line>> {
    let middle = |r: &Rect| r.y + r.h / 2;
    let mut out = Vec::new();
    for s in stacks {
        if s.len() == 1 {
            out.push(s.clone());
            continue;
        }
        let r = s.iter().skip(1).fold(s[0].rect, |r, l| r.union(&l.rect));
        let mut beside: Vec<(u32, u32)> = stacks
            .iter()
            .filter(|o| o.len() == 1)
            .map(|o| &o[0].rect)
            .filter(|o| {
                (o.right() <= r.x || o.x >= r.right()) && (r.y..r.bottom()).contains(&middle(o))
            })
            .map(|o| (middle(o), o.h))
            .collect();
        beside.sort_unstable();
        // The middles of the rows beside, one per row.
        let mut rows: Vec<u32> = Vec::new();
        for (m, h) in beside {
            if rows.last().is_none_or(|last| m > last + h / 2) {
                rows.push(m);
            }
        }
        let cuts: Vec<u32> = rows.windows(2).map(|w| (w[0] + w[1]) / 2).collect();
        let mut cells: Vec<Vec<&Line>> = vec![Vec::new(); cuts.len() + 1];
        for l in s {
            let m = middle(&l.rect);
            cells[cuts.iter().filter(|c| **c <= m).count()].push(*l);
        }
        out.extend(cells.into_iter().filter(|c| !c.is_empty()));
    }
    out
}

/// How much two boxes overlap vertically, as a share of the lower one.
fn overlap_y(a: &Rect, b: &Rect) -> f32 {
    let top = a.y.max(b.y);
    let bottom = a.bottom().min(b.bottom());
    bottom.saturating_sub(top) as f32 / a.h.min(b.h).max(1) as f32
}

/// Whether `lines[i]` is in a table's row of heads: on top of its column,
/// in a row of three or more lines that head a column. Not `heads[i]`: the
/// head of an empty column has nothing below it. A form's label and value
/// head two columns only.
fn in_head_row(lines: &[Line], heads: &[bool], i: usize) -> bool {
    column(lines, i, true).next().is_none()
        && (0..lines.len())
            .filter(|&j| heads[j] && overlap_y(&lines[j].rect, &lines[i].rect) >= 0.4)
            .count()
            >= 3
}

/// What stands next to a line: the nearest line on its left in the same
/// row and the nearest line above it that overlaps it horizontally; after
/// them, on a line of its own, the head of its table's column. A form puts
/// the label there (`Geburtsdatum  14.03.1987`); in a two-column layout the
/// rows of the page do not always pair them. A table puts it on top of the
/// column (`Gegner` above a row of surnames), and the line above is the row
/// before; a rule at 0 reads the head only (`detect::Detector::candidates`).
/// A table's head has no label: its left neighbour is another head.
fn neighbours(lines: &[Line], heads: &[bool], i: usize) -> String {
    let l = &lines[i].rect;
    let is_head = in_head_row(lines, heads, i);
    let left = lines
        .iter()
        .enumerate()
        .filter(|(j, o)| *j != i && o.rect.right() <= l.x + 2 && overlap_y(&o.rect, l) >= 0.4)
        .filter(|_| !is_head)
        .min_by_key(|(_, o)| l.x.saturating_sub(o.rect.right()));
    let above = lines
        .iter()
        .enumerate()
        .filter(|(j, o)| {
            *j != i && o.rect.bottom() <= l.y + 2 && o.rect.x < l.right() && l.x < o.rect.right()
        })
        .min_by_key(|(_, o)| l.y.saturating_sub(o.rect.bottom()));
    let head = column(lines, i, true)
        .min_by_key(|&j| lines[j].rect.y)
        .filter(|&j| in_head_row(lines, heads, j))
        .map_or("", |j| lines[j].text.as_str());
    let near = [above, left]
        .into_iter()
        .flatten()
        .map(|(_, o)| o.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    // The head's line stays when it is empty: the last line is the head.
    if near.is_empty() && head.is_empty() {
        String::new()
    } else {
        format!("{near}\n{head}")
    }
}

/// The boxes of the characters in `start..end`, one per line, padded.
fn boxes(p: &Page, start: usize, end: usize, width: u32, height: u32) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    for (s, e, r) in &p.chars {
        if *s < end && *e > start {
            match out.iter_mut().find(|o| o.y == r.y && o.h == r.h) {
                Some(o) => *o = o.union(r),
                None => out.push(*r),
            }
        }
    }
    out.into_iter().map(|r| r.pad(PAD, width, height)).collect()
}

/// Whether `r` lies (nearly) inside one of `done`. Touching is not enough:
/// a padded box reaches a few pixels into the line below it
/// (`Jonas Brandt` above `jonas.brandt@exam…`), and the value there would
/// stay visible.
fn covered<'a>(r: &Rect, mut done: impl Iterator<Item = &'a Rect>) -> bool {
    let area = u64::from(r.w) * u64::from(r.h);
    done.any(|f| {
        let w = r.right().min(f.right()).saturating_sub(r.x.max(f.x));
        let h = r.bottom().min(f.bottom()).saturating_sub(r.y.max(f.y));
        // Nine tenths: OCR reads a box a pixel or two off the second time.
        10 * u64::from(w) * u64::from(h) >= 9 * area
    })
}

fn fill(img: &mut RgbaImage, r: &Rect) {
    for y in r.y..r.bottom().min(img.height()) {
        for x in r.x..r.right().min(img.width()) {
            img.put_pixel(x, y, Rgba([0, 0, 0, 255]));
        }
    }
}

/// Redacts one picture: the boxes filled (with `apply`) and their findings
/// reported, at `frame` (a GIF's frame or a PDF's page, from 1).
pub fn redact_picture(
    engine: &Engine,
    ocr: &Ocr,
    img: &mut RgbaImage,
    frame: Option<u64>,
    apply: bool,
    out: &mut Outcome,
) -> Result<Vec<Rect>, Error> {
    let mut boxes = faces(engine, img, frame, apply, out, &[])?;
    boxes.extend(read_whole(engine, ocr, img, frame, apply, out)?);
    Ok(boxes)
}

/// The text of a whole picture, redacted: [`settle`] without a band.
fn read_whole(
    engine: &Engine,
    ocr: &Ocr,
    img: &mut RgbaImage,
    frame: Option<u64>,
    apply: bool,
    out: &mut Outcome,
) -> Result<Vec<Rect>, Error> {
    settle(engine, ocr, img, frame, apply, out, Band::WHOLE)?
        .ok_or_else(|| error("a whole picture has no edge a line could be cut at"))
}

/// The faces in `img`, reported as `FACE` and filled (with `apply`), at
/// `frame`; nothing when the policy keeps faces. A face inside a box of
/// `kept` (a box of the frame before, whose pixels stood still) is that
/// box's and not reported again. The boxes filled are returned.
fn faces(
    engine: &Engine,
    img: &mut RgbaImage,
    frame: Option<u64>,
    apply: bool,
    out: &mut Outcome,
    kept: &[Rect],
) -> Result<Vec<Rect>, Error> {
    let Some(detector) = engine.faces() else {
        return Ok(Vec::new());
    };
    let entity = EntityType::new("FACE").expect("a valid type");
    let threshold = engine.threshold(&entity);
    let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let mut filled = Vec::new();
    for face in detector.find(&rgb)? {
        let confidence = crate::face::confidence(face.score);
        if confidence < threshold || covered(&face.rect, kept.iter()) {
            continue;
        }
        out.findings.push(Finding {
            entity: entity.clone(),
            location: Location::Region {
                part: None,
                frame,
                x: face.rect.x,
                y: face.rect.y,
                width: face.rect.w,
                height: face.rect.h,
            },
            confidence,
            detector: "face".into(),
            pseudonym: None,
        });
        if apply {
            fill(img, &face.rect);
        }
        filled.push(face.rect);
    }
    if !filled.is_empty() {
        if !out.review_reasons.iter().any(|r| r == FACES_FOUND) {
            out.review_reasons.push(FACES_FOUND.into());
        }
        if matches!(
            engine.action_for_entity(&entity),
            crate::Action::Pseudonymize | crate::Action::Fake
        ) && !out.review_reasons.iter().any(|r| r == BOXES_ONLY)
        {
            out.review_reasons.push(BOXES_ONLY.into());
        }
    }
    Ok(filled)
}

/// Where a picture being read is a band of a larger one: its top row there,
/// and whether rows above or below it were left out (a line there may be
/// cut by the band's edge).
#[derive(Clone, Copy)]
struct Band {
    top: u32,
    open_above: bool,
    open_below: bool,
}

impl Band {
    const WHOLE: Band = Band {
        top: 0,
        open_above: false,
        open_below: false,
    };
}

/// How close to an open edge a line's box may come before it counts as cut.
const EDGE: u32 = 2;

/// `text` with the formatting marks of a word processor read as text taken
/// for spaces: a space mark `·` anywhere, and a `-` between two letters
/// (OCR reads the mark as one). Byte for byte as long as `text`, so a
/// finding's range is the same in both and its boxes stay its own. `None`
/// when there is nothing to change.
fn unmarked(text: &str) -> Option<String> {
    if !text.contains(['·', '-']) {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    for (i, &c) in chars.iter().enumerate() {
        let between = |f: fn(&char) -> bool| {
            i > 0 && i + 1 < chars.len() && f(&chars[i - 1]) && f(&chars[i + 1])
        };
        let between_letters = between(|c| c.is_alphabetic());
        // A space mark takes a space's place, so it sits right between two
        // words; a separator (`Lindenweg 12 · 28195 Bremen`) has spaces
        // around it and stays.
        let between_words = between(|c| c.is_alphanumeric());
        match c {
            // Two bytes in UTF-8, two spaces.
            '·' if between_words => {
                out.push_str("  ");
                changed = true;
            }
            '-' if between_letters => {
                out.push(' ');
                changed = true;
            }
            c => out.push(c),
        }
    }
    debug_assert_eq!(out.len(), text.len());
    changed.then_some(out)
}

/// What the detectors find in the lines OCR read: the page as one text,
/// and each line with its neighbours as context; each with its boxes. A
/// text that may hold a word processor's formatting marks (`Sven·Heid`,
/// read as `Sven-Heid`) is read a second time with them as spaces: a
/// joined name is not one word, and a finding of either reading is filled.
fn candidates(
    engine: &Engine,
    lines: &[Line],
    w: u32,
    h: u32,
    again: bool,
) -> Result<Vec<(crate::detect::Candidate, Vec<Rect>)>, Error> {
    let read = |text: &str, context: &str| {
        if again {
            engine.ocr_findings_again(text, context)
        } else {
            engine.ocr_findings(text, context)
        }
    };
    let p = page(lines);
    let mut out = Vec::new();
    // The names the page holds: a line read alone does not know them, and
    // `Brandts Kapuze` there is cut to `Brandts` by them.
    let mut names: Vec<String> = Vec::new();
    for text in std::iter::once(p.text.clone()).chain(unmarked(&p.text)) {
        for c in read(&text, "")? {
            if c.entity.as_str() == "PERSON" {
                names.extend(crate::detect::name_words(&text[c.start..c.end]).map(str::to_owned));
            }
            let rects = boxes(&p, c.start, c.end, w, h);
            out.push((c, rects));
        }
    }
    let heads = heads(lines);
    for (i, line) in lines.iter().enumerate() {
        let one = Page {
            text: line.text.clone(),
            chars: line.chars.clone(),
        };
        let context = neighbours(lines, &heads, i);
        for text in std::iter::once(line.text.clone()).chain(unmarked(&line.text)) {
            for mut c in read(&text, &context)? {
                if c.entity.as_str() == "PERSON" {
                    let span = &text[c.start..c.end];
                    let own: Vec<&str> = crate::detect::name_words(span).collect();
                    let others = names
                        .iter()
                        .map(String::as_str)
                        .filter(|n| !own.contains(n));
                    if let Some(n) = crate::detect::genitive_head(span, others) {
                        c.end = c.start + n;
                    }
                }
                let rects = boxes(&one, c.start, c.end, w, h);
                out.push((c, rects));
            }
        }
    }
    Ok(out)
}

/// [`redact_picture`] on `img`, a band of a picture: the findings are
/// reported where they are in the picture, the boxes returned are in the
/// band. `None` when OCR reads a line at an edge of the band that was left
/// out: the line may be cut there, and the band must be read larger.
fn settle(
    engine: &Engine,
    ocr: &Ocr,
    img: &mut RgbaImage,
    frame: Option<u64>,
    apply: bool,
    out: &mut Outcome,
    band: Band,
) -> Result<Option<Vec<Rect>>, Error> {
    let (w, h) = img.dimensions();
    let mut filled: Vec<Rect> = Vec::new();
    for round in 1..=ROUNDS {
        let before = out.findings.len();
        let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
        let lines = ocr.read(&rgb)?;
        if lines.iter().any(|l| {
            (band.open_above && l.rect.y <= EDGE)
                || (band.open_below && l.rect.bottom() + EDGE >= h)
        }) {
            return Ok(None);
        }
        let candidates = candidates(engine, &lines, w, h, round > 1)?;
        let mut new: Vec<Rect> = Vec::new();
        for (c, rects) in candidates {
            // Found twice (page and line), or inside a box already filled.
            if rects.is_empty() || rects.iter().all(|r| covered(r, filled.iter().chain(&new))) {
                continue;
            }
            let bound = rects.iter().skip(1).fold(rects[0], |a, r| a.union(r));
            if matches!(
                engine.action_for(&c),
                crate::Action::Pseudonymize | crate::Action::Fake
            ) && !out.review_reasons.iter().any(|r| r == BOXES_ONLY)
            {
                out.review_reasons.push(BOXES_ONLY.into());
            }
            out.findings.push(Finding {
                entity: c.entity.clone(),
                location: Location::Region {
                    part: None,
                    frame,
                    x: bound.x,
                    y: bound.y + band.top,
                    width: bound.w,
                    height: bound.h,
                },
                confidence: c.confidence,
                detector: c.detector.clone(),
                pseudonym: None,
            });
            new.extend(rects);
        }
        // Read badly where nothing is filled: on the first reading only, as
        // a later one reads the boxes' edges.
        if round == 1 {
            let unread = unread(engine, &lines, &new, &band, frame, (w, h), out)?;
            new.extend(unread);
        }
        if new.is_empty() || !apply {
            return Ok(Some(filled));
        }
        let last = round == ROUNDS;
        if last && !engine.unverified() {
            let f = &out.findings[before];
            return Err(Error::Unsettled {
                entity: f.entity.as_str().to_owned(),
                at: place(f),
                rounds: ROUNDS,
            });
        }
        for r in &new {
            fill(img, r);
        }
        filled.extend(new);
        if last {
            let found = &out.findings[before..];
            let more = match found.len() {
                1 => String::new(),
                n => format!(" and {} more", n - 1),
            };
            out.review_reasons.push(format!(
                "the picture was written unverified: after {ROUNDS} rounds OCR still found {} at {}{more}; that is filled too, but the picture was not read again",
                found[0].entity.as_str(),
                place(&found[0]),
            ));
        }
    }
    Ok(Some(filled))
}

/// The lines OCR read badly where nothing in `new` covers them, as the
/// policy says (ADR-0045): `review` notes each for the report's reason,
/// `box` reports each as `UNREAD_TEXT` and returns its box to fill, `error`
/// stops at the first.
fn unread(
    engine: &Engine,
    lines: &[Line],
    new: &[Rect],
    band: &Band,
    frame: Option<u64>,
    (w, h): (u32, u32),
    out: &mut Outcome,
) -> Result<Vec<Rect>, Error> {
    let low = engine.policy().picture.low_confidence.unwrap_or_default();
    let mut boxes = Vec::new();
    for l in lines {
        if !l.weak().any(|r| !covered(r, new.iter())) {
            continue;
        }
        let r = Rect {
            y: l.rect.y + band.top,
            ..l.rect
        };
        match low {
            LowConfidence::Review => out.weak_lines.push(at(&r, frame)),
            LowConfidence::Error => return Err(Error::Unread { at: at(&r, frame) }),
            LowConfidence::Box => {
                out.findings.push(Finding {
                    entity: EntityType::new(UNREAD_TEXT).expect("a valid type"),
                    location: Location::Region {
                        part: None,
                        frame,
                        x: r.x,
                        y: r.y,
                        width: r.w,
                        height: r.h,
                    },
                    confidence: 1.0,
                    detector: UNREAD_DETECTOR.into(),
                    pseudonym: None,
                });
                boxes.push(l.rect.pad(PAD, w, h));
            }
        }
    }
    Ok(boxes)
}

/// The format of an image file, from its bytes.
fn format_of(bytes: &[u8]) -> Result<ImageFormat, Error> {
    image::guess_format(bytes).map_err(|_| error("not an image veilq reads (PNG, JPEG, WebP, GIF)"))
}

fn outcome() -> Outcome {
    Outcome {
        output: Vec::new(),
        findings: Vec::new(),
        mapping: Vec::new(),
        review_reasons: Vec::new(),
        weak_lines: Vec::new(),
    }
}

/// Whether the pixels inside `r` differ between `a` and `b`.
fn changed(a: &RgbaImage, b: &RgbaImage, r: &Rect) -> bool {
    (r.y..r.bottom().min(a.height()))
        .any(|y| (r.x..r.right().min(a.width())).any(|x| a.get_pixel(x, y) != b.get_pixel(x, y)))
}

/// Rows read around a changed row, above and below, at least: a label above
/// a value, the rest of a line that changed in part (#12).
const MARGIN: u32 = 64;

/// The bands of rows, top to bottom, where `b` differs from `a`, each
/// `margin` rows wider on both sides, the overlapping ones joined. A band is
/// as wide as the picture, so a label left of a value is in it.
fn changed_bands(a: &RgbaImage, b: &RgbaImage, margin: u32) -> Vec<(u32, u32)> {
    let (w, h) = a.dimensions();
    let row = 4 * w as usize;
    let mut out = Vec::new();
    for (y, (ra, rb)) in (0u32..).zip(a.as_raw().chunks(row).zip(b.as_raw().chunks(row))) {
        if ra != rb {
            out.push((y.saturating_sub(margin), (y + 1 + margin).min(h)));
        }
    }
    joined(out)
}

/// Bands sorted and those that overlap or touch joined.
fn joined(mut bands: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    bands.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::new();
    for (top, bottom) in bands {
        match out.last_mut() {
            Some(last) if top <= last.1 => last.1 = last.1.max(bottom),
            _ => out.push((top, bottom)),
        }
    }
    out
}

/// Whether bands of this many rows are worth reading on their own: above
/// half the picture (a scroll, a new page), reading it whole costs no more.
fn too_much(bands: &[(u32, u32)], height: u32) -> bool {
    2 * bands.iter().map(|(t, b)| b - t).sum::<u32>() > height
}

/// A GIF frame that changed against the one before, read only in the bands
/// where its pixels changed (#12). A band whose edge cuts a line OCR reads
/// is read again, larger; when the bands come to more than half the frame,
/// it is read whole. Outside the bands every box of the frame before stays,
/// as its pixels stand still (`kept`). Faces are looked for in the whole
/// frame: a face cut by a band's edge is still one. The boxes returned are
/// in the frame.
#[allow(clippy::too_many_arguments)]
fn redact_changed(
    engine: &Engine,
    ocr: &Ocr,
    before: &RgbaImage,
    after: &RgbaImage,
    img: &mut RgbaImage,
    frame: Option<u64>,
    apply: bool,
    out: &mut Outcome,
    kept: &[Rect],
) -> Result<Vec<Rect>, Error> {
    let (w, h) = img.dimensions();
    let mut boxes = faces(engine, img, frame, apply, out, kept)?;
    let margin = MARGIN.max(h / 20);
    let mut bands = changed_bands(before, after, margin);
    // Read each band on a copy; a band cut short starts the reading over, so
    // nothing of a reading given up reaches the frame or the report.
    let read = 'read: loop {
        if too_much(&bands, h) {
            boxes.extend(read_whole(engine, ocr, img, frame, apply, out)?);
            return Ok(boxes);
        }
        let mut read = Vec::new();
        for (i, &(top, bottom)) in bands.iter().enumerate() {
            let mut part = image::imageops::crop_imm(img, 0, top, w, bottom - top).to_image();
            let mut found = outcome();
            let band = Band {
                top,
                open_above: top > 0,
                open_below: bottom < h,
            };
            match settle(engine, ocr, &mut part, frame, apply, &mut found, band)? {
                Some(rects) => read.push((top, part, found, rects)),
                None => {
                    // Larger on the open sides; each time by `margin` at
                    // least, so the bands come to the whole frame in the end.
                    bands[i] = (top.saturating_sub(margin), (bottom + margin).min(h));
                    bands = joined(bands);
                    continue 'read;
                }
            }
        }
        break read;
    };
    for (top, part, found, rects) in read {
        if apply {
            image::imageops::replace(img, &part, 0, i64::from(top));
        }
        out.findings.extend(found.findings);
        out.weak_lines.extend(found.weak_lines);
        for r in found.review_reasons {
            if !out.review_reasons.contains(&r) {
                out.review_reasons.push(r);
            }
        }
        boxes.extend(rects.into_iter().map(|r| Rect { y: r.y + top, ..r }));
    }
    Ok(boxes)
}

/// An animated GIF, frame by frame (frames counted from 1), encoded anew.
fn gif(engine: &Engine, ocr: &Ocr, bytes: &[u8], apply: bool) -> Result<Outcome, Error> {
    // Frame by frame, decoded and encoded as they come: a screen recording
    // of a few hundred frames at 4K is tens of GB decoded at once.
    let frames = GifDecoder::new(Cursor::new(bytes))
        .map_err(error)?
        .into_frames();
    let mut out = outcome();
    let mut buf = Vec::new();
    let mut encoder = if apply {
        let mut e = GifEncoder::new_with_speed(&mut buf, 10);
        e.set_repeat(Repeat::Infinite).map_err(error)?;
        Some(e)
    } else {
        None
    };
    // The previous frame as decoded, as redacted, and its boxes.
    let mut prev: Option<(RgbaImage, RgbaImage)> = None;
    let mut boxes: Vec<Rect> = Vec::new();
    for (i, frame) in frames.enumerate() {
        let frame = frame.map_err(error)?;
        let delay = frame.delay();
        let original = frame.into_buffer();
        let img = match &prev {
            Some((before, done)) if *before == original => done.clone(),
            _ => {
                let mut img = original.clone();
                // A box stays while its pixels stand still.
                if let Some((before, _)) = &prev {
                    boxes.retain(|r| !changed(before, &original, r));
                } else {
                    boxes.clear();
                }
                if apply {
                    for r in &boxes {
                        fill(&mut img, r);
                    }
                }
                let n = Some(i as u64 + 1);
                let found = match &prev {
                    Some((before, _)) => redact_changed(
                        engine, ocr, before, &original, &mut img, n, apply, &mut out, &boxes,
                    )?,
                    None => redact_picture(engine, ocr, &mut img, n, apply, &mut out)?,
                };
                boxes.extend(found);
                img
            }
        };
        if let Some(e) = encoder.as_mut() {
            e.encode_frame(Frame::from_parts(img.clone(), 0, 0, delay))
                .map_err(error)?;
        }
        prev = Some((original, img));
    }
    drop(encoder);
    out.output = buf;
    weak_reason(&mut out);
    Ok(out)
}

/// A PNG, JPEG, WebP or GIF image, redacted and encoded anew (no metadata).
pub fn image(engine: &Engine, ocr: &Ocr, bytes: &[u8], apply: bool) -> Result<Outcome, Error> {
    let format = format_of(bytes)?;
    if format == ImageFormat::Gif {
        return gif(engine, ocr, bytes, apply);
    }
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format)
        .into_decoder()
        .map_err(error)?;
    let orientation = decoder.orientation().map_err(error)?;
    let exif = decoder
        .exif_metadata()
        .ok()
        .flatten()
        .is_some_and(|e| !e.is_empty());
    let mut img = DynamicImage::from_decoder(decoder).map_err(error)?;
    img.apply_orientation(orientation);
    let mut rgba = img.to_rgba8();
    let mut out = outcome();
    if exif {
        out.findings.push(Finding {
            entity: EntityType::new("METADATA").expect("a valid type"),
            location: Location::Region {
                part: None,
                frame: None,
                x: 0,
                y: 0,
                width: rgba.width(),
                height: rgba.height(),
            },
            confidence: 1.0,
            detector: "metadata".into(),
            pseudonym: None,
        });
    }
    redact_picture(engine, ocr, &mut rgba, None, apply, &mut out)?;
    if apply {
        out.output = encode(&rgba, format)?;
    }
    weak_reason(&mut out);
    Ok(out)
}

/// `img` in `format`: JPEG at quality 90 (without alpha), PNG and WebP
/// lossless. Nothing but pixels is written.
pub fn encode(img: &RgbaImage, format: ImageFormat) -> Result<Vec<u8>, Error> {
    let mut buf = Cursor::new(Vec::new());
    match format {
        ImageFormat::Jpeg => {
            let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 90)
                .encode_image(&rgb)
                .map_err(error)?;
        }
        ImageFormat::Png | ImageFormat::WebP => {
            DynamicImage::ImageRgba8(img.clone())
                .write_to(&mut buf, format)
                .map_err(error)?;
        }
        other => return Err(error(format!("veilq does not write {other:?}"))),
    }
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x: u32, y: u32) -> Line {
        let chars = text
            .char_indices()
            .enumerate()
            .map(|(i, (b, c))| {
                (
                    b,
                    b + c.len_utf8(),
                    Rect {
                        x: x + 10 * i as u32,
                        y,
                        w: 10,
                        h: 20,
                    },
                )
            })
            .collect();
        Line {
            text: text.into(),
            rect: Rect {
                x,
                y,
                w: 10 * text.chars().count() as u32,
                h: 20,
            },
            scores: vec![1.0; text.chars().count()],
            chars,
            score: 1.0,
        }
    }

    #[test]
    fn a_line_read_badly_is_one_review_reason_per_file() {
        let mut l = line("Lena Albrecht", 0, 0);
        assert_eq!(l.weak().count(), 0);
        l.scores[5] = 0.6;
        assert_eq!(l.weak().collect::<Vec<_>>(), vec![&l.chars[5].2]);
        // A separator read badly holds no personal data; a line of one
        // character is an icon or a cursor.
        let mut sep = line("Lindenweg 12 · Bremen", 0, 0);
        sep.scores[13] = 0.5;
        assert_eq!(sep.weak().count(), 0);
        let mut one = line("e", 0, 0);
        one.scores[0] = 0.3;
        assert_eq!(one.weak().count(), 0);

        let mut out = outcome();
        weak_reason(&mut out);
        assert!(out.review_reasons.is_empty(), "nothing read badly");
        out.weak_lines = vec![
            "130x20 at 0,0".into(),
            "80x20 at 0,40 (frame or page 2)".into(),
        ];
        weak_reason(&mut out);
        assert_eq!(
            out.review_reasons,
            vec![
                "OCR read 2 lines with low confidence, the first at 130x20 at 0,0 (handwriting, small or blurred text): personal data there may not have been found"
            ]
        );
        assert!(out.weak_lines.is_empty());
    }

    #[test]
    fn lines_become_rows_and_findings_become_boxes() {
        // A label and its value on one row, another row below.
        let lines = [
            line("Geburtsdatum", 0, 0),
            line("14.03.1987", 200, 2),
            line("Kfz HB-MA 412", 0, 40),
        ];
        let p = page(&lines);
        assert_eq!(p.text, "Geburtsdatum  14.03.1987\nKfz HB-MA 412");
        let start = p.text.find("14.03").unwrap();
        let b = boxes(&p, start, start + 10, 1000, 1000);
        assert_eq!(
            b,
            vec![Rect {
                x: 198,
                y: 0,
                w: 104,
                h: 24
            }]
        );
    }

    #[test]
    fn a_cell_of_a_table_is_read_whole() {
        // Two rows of a table, as OCR reads them: the holder's cell is full,
        // its lines sit as close to the next row's as to each other, and the
        // boxes overlap by a few pixels. The amounts beside mark the rows.
        let row = |y: u32, amount: &str| {
            [
                line("Nordwerk", 0, y + 8),
                line("Inkasso GmbH", 0, y + 24),
                line("KMS Bad", 400, y),
                line("Kissinger", 390, y + 17),
                line("Servicegesellschaft", 340, y + 34),
                line(amount, 800, y + 18),
            ]
        };
        let mut lines = row(100, "2.247,43").to_vec();
        lines.extend(row(152, "1.661,55"));
        let p = page(&lines);
        let row = "Nordwerk Inkasso GmbH  KMS Bad Kissinger Servicegesellschaft";
        assert_eq!(
            p.text,
            format!("{row}  2.247,43\n{row}  1.661,55"),
            "{}",
            p.text
        );
        // A finding keeps its boxes, one per line.
        let start = p.text.find("Kissinger").unwrap();
        assert_eq!(boxes(&p, start, start + 9, 2000, 2000)[0].y, 115);

        // Lines further apart than half a line stay rows of their own.
        let lines = [line("Mira Albers", 0, 0), line("Lindenweg 12", 0, 40)];
        assert_eq!(page(&lines).text, "Mira Albers\nLindenweg 12");
    }

    #[test]
    fn a_cell_of_a_table_reads_the_head_of_its_column() {
        // A list as OCR reads it: a row of heads, then rows whose surname
        // and street stand in cells of their own; one row has no surname.
        let mut lines = vec![
            line("Mandant", 0, 0),
            line("Gegner", 200, 0),
            line("Status", 340, 0),
            line("Straße", 560, 0),
            line("PLZ", 700, 0),
        ];
        for (k, (name, gegner, street, plz)) in [
            ("Kerstin Volkmann", "Brauner", "Eichbreite", "31785"),
            ("Jana Pohl", "", "Am Güterba ...", "56564"),
            ("Arne Kowalski", "Dietz", "Hachenburg ...", "65936"),
        ]
        .into_iter()
        .enumerate()
        {
            let y = 30 * (k as u32 + 1);
            lines.extend([line(name, 0, y), line(street, 560, y), line(plz, 700, y)]);
            if !gegner.is_empty() {
                lines.push(line(gegner, 200, y));
            }
        }
        let at = |text: &str| lines.iter().rposition(|l| l.text == text).unwrap();
        let heads = heads(&lines);
        // The head comes last, on a line of its own: the row before is
        // above it.
        assert!(neighbours(&lines, &heads, at("Dietz")).ends_with("\nGegner"));
        // A head's left neighbour in the row of heads is no label.
        assert!(!neighbours(&lines, &heads, at("Status")).contains("Gegner"));

        let engine = Engine::new(&crate::Policy::default(), None, b"test", None).unwrap();
        let found = |text: &str| {
            let i = at(text);
            engine
                .ocr_findings(text, &neighbours(&lines, &heads, i))
                .unwrap()
                .into_iter()
                .map(|c| c.entity.as_str().to_owned())
                .collect::<Vec<_>>()
        };
        for text in ["Brauner", "Dietz"] {
            assert_eq!(found(text), ["PERSON"], "{text}");
        }
        for text in ["Eichbreite", "Am Güterba ...", "Hachenburg ..."] {
            assert_eq!(found(text), ["ADDRESS"], "{text}");
        }
        for text in ["Gegner", "Status", "Straße", "31785"] {
            assert!(found(text).is_empty(), "{text}: {:?}", found(text));
        }

        // A form's labels stand in a column too, but head no table: the
        // label above is no column's name.
        let lines = [
            line("Antrag auf Erstattung", 0, 0),
            line("Anschrift", 0, 40),
            line("Goethestraße 86, 10115 Berlin", 200, 40),
            line("Telefon", 0, 70),
            line("030 5550123", 200, 70),
        ];
        let heads = super::heads(&lines);
        let context = neighbours(&lines, &heads, 3);
        assert!(
            engine.ocr_findings("Telefon", &context).unwrap().is_empty(),
            "{context:?}"
        );
    }

    #[test]
    fn formatting_marks_read_as_dashes_do_not_hide_a_name() {
        // A Word screenshot with its formatting marks shown: OCR reads the
        // space marks as `-` or `·` and a pilcrow as `l`.
        let engine = Engine::new(&crate::Policy::default(), None, b"test", None).unwrap();
        let found = |text: &str| {
            let lines = [line(text, 0, 0)];
            candidates(&engine, &lines, 2000, 100, false)
                .unwrap()
                .into_iter()
                .map(|(c, _)| {
                    (
                        c.entity.as_str().to_owned(),
                        text[c.start..c.end].to_owned(),
                    )
                })
                .collect::<Vec<_>>()
        };
        for text in [
            "Rückruf-an-Herr-Jonas-Brandt-heute",
            "Rückruf·an·Herr·Jonas·Brandt·heute",
        ] {
            assert!(
                found(text)
                    .iter()
                    .any(|(e, v)| e == "PERSON" && v.contains("Jonas")),
                "{text}: {:?}",
                found(text)
            );
        }
        // A real compound stays as it is: its parts are no names.
        assert!(
            found("Rechnung-an-Baden-Württemberg")
                .iter()
                .all(|(e, _)| e != "PERSON")
        );
        // The text keeps its length: the boxes of a finding stay its own.
        assert_eq!(unmarked("a·b-c").unwrap(), "a  b c");
        assert_eq!(unmarked("ohne Marken"), None);
        // A separator in a letterhead is no space mark: an address after it
        // is not joined to what stands before.
        assert_eq!(
            unmarked("Kanzlei Muster · Lindenweg 12 · 28195 Bremen"),
            None
        );
    }

    /// With the real NER model (VEILQ_NER_DIR, VEILQ_ORT_DYLIB): a line
    /// read alone takes what a name owns for the name; the names of the
    /// page cut it off.
    #[cfg(feature = "ner")]
    #[test]
    fn a_line_does_not_take_what_a_name_owns() {
        let (Ok(dir), Ok(dylib)) = (
            std::env::var("VEILQ_NER_DIR"),
            std::env::var("VEILQ_ORT_DYLIB"),
        ) else {
            return;
        };
        let dir = std::path::Path::new(&dir);
        let ner = crate::ner::Ner::load(&crate::ner::ModelFiles {
            model: &dir.join("model.onnx"),
            tokenizer: &dir.join("tokenizer.json"),
            runtime: std::path::Path::new(&dylib),
        })
        .unwrap();
        let engine = Engine::new(&crate::Policy::default(), None, b"test", None)
            .unwrap()
            .with_ner(std::sync::Arc::new(ner));
        let lines = [
            line(
                "Mira Albers trifft Jonas Brandt: alle starrten auf die Kapuze",
                0,
                0,
            ),
            line(
                "Brandts Kapuze, seine Hände und einen vierfarbigen Kugelschreiber.",
                0,
                40,
            ),
        ];
        let found: Vec<Rect> = candidates(&engine, &lines, 2000, 100, false)
            .unwrap()
            .into_iter()
            .filter(|(c, _)| c.entity.as_str() == "PERSON")
            .flat_map(|(_, rects)| rects)
            .collect();
        // `Mira Albers` on the first line; on the second, `Brandts` is
        // characters 0 to 7 (x 0 to 70), `Kapuze` starts at x 80.
        assert!(found.iter().any(|r| r.y < 20 && r.x == 0), "{found:?}");
        assert!(found.iter().any(|r| r.y >= 38), "{found:?}");
        assert!(
            found
                .iter()
                .filter(|r| r.y >= 38)
                .all(|r| r.right() <= 72 + PAD),
            "{found:?}"
        );
    }

    #[test]
    fn a_box_that_touches_a_value_does_not_cover_it() {
        let r = |x, y, w, h| Rect { x, y, w, h };
        // A name's padded box above, reaching 5 pixels into the address.
        let name = r(51, 1328, 69, 25);
        let address = r(54, 1348, 104, 19);
        assert!(!covered(&address, [name].iter()));
        // The same value found twice, or read a pixel off in the next round.
        assert!(covered(&address, [address].iter()));
        assert!(covered(&r(55, 1349, 104, 19), [address].iter()));
        assert!(!covered(&address, std::iter::empty()));
    }

    #[test]
    fn an_unsettled_picture_says_where() {
        let f = Finding {
            entity: EntityType::new("PERSON").unwrap(),
            location: Location::Region {
                part: None,
                frame: Some(3),
                x: 142,
                y: 228,
                width: 97,
                height: 24,
            },
            confidence: 0.8,
            detector: "ner".into(),
            pseudonym: None,
        };
        assert_eq!(place(&f), "97x24 at 142,228 (frame or page 3)");
        let e = Error::Unsettled {
            entity: "PERSON".into(),
            at: place(&f),
            rounds: ROUNDS,
        };
        let m = e.to_string();
        assert!(
            m.contains("PERSON at 97x24 at 142,228") && m.contains("--unverified"),
            "{m}"
        );
    }

    #[test]
    fn a_box_stays_while_its_pixels_stand_still() {
        let a = RgbaImage::from_pixel(20, 20, Rgba([255, 255, 255, 255]));
        let mut b = a.clone();
        b.put_pixel(15, 15, Rgba([0, 0, 0, 255]));
        let still = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        };
        let moved = Rect {
            x: 10,
            y: 10,
            w: 10,
            h: 10,
        };
        assert!(!changed(&a, &b, &still));
        assert!(changed(&a, &b, &moved));
    }

    #[test]
    fn a_frame_is_read_in_bands_around_what_changed() {
        let a = RgbaImage::from_pixel(50, 1000, Rgba([255, 255, 255, 255]));
        let mut b = a.clone();
        // A pointer near the top, a word typed lower down, a clock in the
        // last row: three bands, as wide as the frame.
        b.put_pixel(3, 10, Rgba([0, 0, 0, 255]));
        b.put_pixel(40, 500, Rgba([0, 0, 0, 255]));
        b.put_pixel(45, 505, Rgba([0, 0, 0, 255]));
        b.put_pixel(0, 999, Rgba([0, 0, 0, 255]));
        assert_eq!(
            changed_bands(&a, &b, 64),
            vec![(0, 75), (436, 570), (935, 1000)]
        );
        assert!(!too_much(&changed_bands(&a, &b, 64), 1000));
        // Bands that come near each other are one.
        assert_eq!(
            joined(vec![(100, 200), (0, 50), (150, 300), (300, 310)]),
            vec![(0, 50), (100, 310)]
        );
        // A scroll changes most rows: read whole.
        let scrolled = RgbaImage::from_fn(50, 1000, |_, y| Rgba([(y % 7) as u8, 0, 0, 255]));
        assert!(too_much(&changed_bands(&a, &scrolled, 64), 1000));
        assert!(changed_bands(&a, &a, 64).is_empty());
    }

    #[test]
    fn encoding_writes_pixels_only() {
        let img = RgbaImage::from_pixel(4, 4, Rgba([200, 10, 10, 255]));
        for f in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            let bytes = encode(&img, f).unwrap();
            assert_eq!(image::guess_format(&bytes).unwrap(), f);
            assert!(!bytes.windows(4).any(|w| w == b"Exif"));
        }
    }

    #[test]
    fn text_read_badly_is_reviewed_filled_or_an_error_as_the_policy_says() {
        let engine = |toml: &str| {
            let policy = crate::Policy::parse(&format!("version = 1\n{toml}")).unwrap();
            Engine::new(&policy, None, b"test", None).unwrap()
        };
        // A handwritten line: one letter read with 0.4. A second line read
        // well, and a third read badly but filled already.
        let mut weak = line("Jonas Brandt", 10, 40);
        weak.scores[2] = 0.4;
        let mut done = line("Mira Albers", 10, 80);
        done.scores[1] = 0.4;
        let lines = [line("Termin am Montag", 10, 0), weak, done];
        let filled = [lines[2].rect.pad(PAD, 400, 200)];
        let run = |e: &Engine| {
            let mut out = outcome();
            let r = unread(e, &lines, &filled, &Band::WHOLE, None, (400, 200), &mut out);
            (r, out)
        };

        let (r, out) = run(&engine(""));
        assert!(r.unwrap().is_empty());
        assert_eq!(out.weak_lines, ["120x20 at 10,40"]);
        assert!(out.findings.is_empty());

        for toml in [
            "[picture]\nlow_confidence = \"box\"",
            "purposes = [\"legal\"]",
        ] {
            let (r, out) = run(&engine(toml));
            assert_eq!(r.unwrap(), [lines[1].rect.pad(PAD, 400, 200)], "{toml}");
            assert!(out.weak_lines.is_empty());
            assert_eq!(out.findings.len(), 1);
            assert_eq!(out.findings[0].entity.as_str(), UNREAD_TEXT);
        }
        // The policy's own value comes before its purposes'.
        let (_, out) = run(&engine(
            "purposes = [\"legal\"]\n[picture]\nlow_confidence = \"review\"",
        ));
        assert!(out.findings.is_empty() && out.weak_lines.len() == 1);

        let (r, _) = run(&engine("[picture]\nlow_confidence = \"error\""));
        let err = r.unwrap_err().to_string();
        assert!(err.contains("120x20 at 10,40"), "{err}");
    }
}
