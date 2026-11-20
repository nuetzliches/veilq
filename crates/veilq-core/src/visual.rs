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
//! Faces (ADR-0038) and signatures (ADR-0048), when the policy has them
//! filled, are `FACE` and `SIGNATURE` findings, found on the picture OCR
//! reads too and filled with the first reading's boxes (ADR-0053). A face
//! in a GIF is looked for in every frame that changed, in the whole frame.
//!
//! A GIF is read frame by frame. A box stays filled in the following frames
//! as long as its pixels do not change there, so a value that stands still
//! is not visible for a frame because OCR missed it once. Unchanged frames
//! are not read again; a frame that changed is read only in bands as wide
//! as the frame around the rows that changed, and whole when those come to
//! more than half of it (ADR-0030). An area a user marked on a frame is
//! filled there, changed or not, and stays like any box (#114).
//!
//! The output is encoded anew, so EXIF, XMP and text chunks are gone; when
//! the input had any (EXIF with camera, time, often GPS; XMP; IPTC; a PNG's
//! text chunks; a JPEG's comments), the report has a `METADATA` finding for
//! it (ADR-0063). EXIF orientation is applied first, so boxes and pixels
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
/// Said when lines read badly were read again as handwriting (#59).
pub const HANDWRITING: &str = "lines read badly were read again as handwriting, and what either reading found was filled; a word misread in both stays: check the handwriting";
/// Said when faces were filled: what the detector missed stays.
pub const FACES_FOUND: &str = "faces were found and filled black; the face detector can miss a face (in profile, small, covered, drawn): check the picture";
/// Said when signatures were filled: what the detector missed stays, and
/// what it took for one (a watermark, a stamp) is filled too.
pub const SIGNATURES_FOUND: &str = "signatures were found and filled black; the signature detector can miss one (small, faint, crossing text) and take a watermark or a stamp for one: check the picture";
/// Verify rounds at most: reading a filled picture again moves rows and
/// labels, so a round can find what the one before did not (ADR-0026).
const ROUNDS: usize = 6;

/// The type of a line OCR read badly, filled with `[picture]
/// low_confidence = "box"` (ADR-0045), and its detector.
pub const UNREAD_TEXT: &str = "UNREAD_TEXT";
const UNREAD_DETECTOR: &str = "ocr:low-confidence";
/// The detector of a line the picture's edge cuts, filled whole (ADR-0059).
const CUT_DETECTOR: &str = "ocr:cut-edge";

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

/// Whether `l` is the next line of `above` in a paragraph set with more room
/// between its lines than [`below`] allows (a headline): under it with a
/// gap of at most a line and a half (OCR's box holds the ink, so a line
/// set at 1.6 times its type size leaves more than a box's height), in the
/// same type size, left aligned.
fn under(above: &Line, l: &Line) -> bool {
    let (a, b) = (&above.rect, &l.rect);
    let h = a.h.min(b.h);
    let close = b.y + h / 4 >= a.bottom() && b.y <= a.bottom() + h * 3 / 2;
    let size = 3 * a.h.max(b.h) <= 4 * h;
    close && size && a.x.abs_diff(b.x) <= h
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
    out.into_iter().map(|r| grown(&r, width, height)).collect()
}

/// `r` with its margin (ADR-0051): a quarter of its height to the sides, an
/// eighth up and down, never less than [`PAD`]. A margin of a few pixels
/// leaves the edges of large or bold letters beside and under the box.
fn grown(r: &Rect, width: u32, height: u32) -> Rect {
    r.pad_xy((r.h / 4).max(PAD), (r.h / 8).max(PAD), width, height)
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
    // Every detector reads the same unfilled picture (#108): a region filled
    // first took the context word a pattern beside it needs. The regions
    // are found and reported here and filled with the first reading's boxes.
    let mut regions = marked(engine, img, frame, out);
    regions.extend(faces(engine, img, frame, out, &[])?);
    regions.extend(signatures(engine, img, frame, out, &[])?);
    let read = read_whole(engine, ocr, img, frame, apply, out, &regions)?;
    regions.extend(read);
    Ok(regions)
}

fn rect(a: &crate::Area) -> Rect {
    Rect {
        x: a.x,
        y: a.y,
        w: a.width,
        h: a.height,
    }
}

/// The areas of `frame` a user marked as personal data (#34): reported as
/// their type (detector `mark`), clipped to the picture. The boxes to fill
/// are returned; they are filled with the first reading's (#108).
fn marked(engine: &Engine, img: &RgbaImage, frame: Option<u64>, out: &mut Outcome) -> Vec<Rect> {
    let (w, h) = img.dimensions();
    let mut filled = Vec::new();
    for (area, entity) in engine
        .picture_marks()
        .missed
        .iter()
        .filter(|(a, _)| a.frame == frame)
    {
        let r = Rect {
            x: area.x.min(w),
            y: area.y.min(h),
            w: area.width.min(w.saturating_sub(area.x)),
            h: area.height.min(h.saturating_sub(area.y)),
        };
        if r.w == 0 || r.h == 0 {
            continue;
        }
        out.findings.push(Finding {
            entity: entity.clone(),
            location: Location::Region {
                part: None,
                frame,
                x: r.x,
                y: r.y,
                width: r.w,
                height: r.h,
            },
            confidence: 1.0,
            detector: MARK_DETECTOR.into(),
            pseudonym: None,
        });
        filled.push(r);
    }
    filled
}

pub use crate::finding::MARK_DETECTOR;

/// Whether `r` (in the picture's pixels, at `frame`) lies in an area a user
/// released (#34): what a detector finds there is left.
fn released(engine: &Engine, frame: Option<u64>, r: &Rect) -> bool {
    let areas: Vec<Rect> = engine
        .picture_marks()
        .released
        .iter()
        .filter(|a| a.frame == frame)
        .map(rect)
        .collect();
    !areas.is_empty() && covered(r, areas.iter())
}

/// The text of a whole picture, redacted: [`settle`] without a band, with
/// `regions` filled with its first reading's boxes.
fn read_whole(
    engine: &Engine,
    ocr: &Ocr,
    img: &mut RgbaImage,
    frame: Option<u64>,
    apply: bool,
    out: &mut Outcome,
    regions: &[Rect],
) -> Result<Vec<Rect>, Error> {
    settle(engine, ocr, img, frame, apply, out, Band::WHOLE, regions)?
        .ok_or_else(|| error("a whole picture has no edge a line could be cut at"))
}

/// The faces in `img`, reported as `FACE`, at `frame`; nothing when the
/// policy keeps faces. A face inside a box of `kept` (a box of the frame
/// before, whose pixels stood still) is that box's and not reported again.
/// The boxes to fill are returned.
fn faces(
    engine: &Engine,
    img: &RgbaImage,
    frame: Option<u64>,
    out: &mut Outcome,
    kept: &[Rect],
) -> Result<Vec<Rect>, Error> {
    let Some(detector) = engine.faces() else {
        return Ok(Vec::new());
    };
    let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let found = detector
        .find(&rgb)?
        .into_iter()
        .map(|f| (f.rect, crate::face::confidence(f.score)))
        .collect();
    let region = Region {
        entity: "FACE",
        detector: "face",
        reason: FACES_FOUND,
    };
    Ok(regions(engine, &region, found, frame, out, kept))
}

/// The handwritten signatures in `img` (ADR-0048), as [`faces`] does with
/// faces: reported as `SIGNATURE`; nothing when the policy keeps
/// signatures.
fn signatures(
    engine: &Engine,
    img: &RgbaImage,
    frame: Option<u64>,
    out: &mut Outcome,
    kept: &[Rect],
) -> Result<Vec<Rect>, Error> {
    let Some(detector) = engine.signatures() else {
        return Ok(Vec::new());
    };
    let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let found = detector
        .find(&rgb)?
        .into_iter()
        .map(|s| (s.rect, crate::signature::confidence(s.score)))
        .collect();
    let region = Region {
        entity: "SIGNATURE",
        detector: "signature",
        reason: SIGNATURES_FOUND,
    };
    Ok(regions(engine, &region, found, frame, out, kept))
}

/// What a detector of regions (faces, signatures) reports.
struct Region {
    entity: &'static str,
    detector: &'static str,
    /// The review reason of a picture in which one was found.
    reason: &'static str,
}

/// `found` (boxes and confidences) reported as findings of `region`, at
/// `frame`. One below its threshold, or inside a box of `kept` (a box of
/// the frame before, whose pixels stood still), is left out. The boxes to
/// fill are returned.
fn regions(
    engine: &Engine,
    region: &Region,
    found: Vec<(Rect, f32)>,
    frame: Option<u64>,
    out: &mut Outcome,
    kept: &[Rect],
) -> Vec<Rect> {
    let entity = EntityType::new(region.entity).expect("a valid type");
    let threshold = engine.threshold(&entity);
    let mut filled = Vec::new();
    for (rect, confidence) in found {
        if confidence < threshold || covered(&rect, kept.iter()) || released(engine, frame, &rect) {
            continue;
        }
        out.findings.push(Finding {
            entity: entity.clone(),
            location: Location::Region {
                part: None,
                frame,
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
            },
            confidence,
            detector: region.detector.into(),
            pseudonym: None,
        });
        filled.push(rect);
    }
    if !filled.is_empty() {
        if !out.review_reasons.iter().any(|r| r == region.reason) {
            out.review_reasons.push(region.reason.into());
        }
        if matches!(
            engine.action_for_entity(&entity),
            crate::Action::Pseudonymize | crate::Action::Fake
        ) && !out.review_reasons.iter().any(|r| r == BOXES_ONLY)
        {
            out.review_reasons.push(BOXES_ONLY.into());
        }
    }
    filled
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

/// The length of an IBAN of `country` as the registry has it
/// (`validate::iban_length`, #89); a country code read wrong as Germany's,
/// where veilq's users bank in most.
fn iban_length(country: &str) -> usize {
    crate::detect::validate::iban_length(country).unwrap_or(22)
}

/// `rects` of a candidate; for an IBAN read short (its checksum fails: OCR
/// lost characters), the last one widened to the right by what the lost
/// characters take, at the width a read one has, a space per four, and one
/// more. Small text runs digits together: the recogniser reads two as one,
/// and the detector ends the line where the gap before the rest looked
/// wide, so the rest of the IBAN stood beside the box unfilled.
fn with_rest(c: &crate::detect::Candidate, value: &str, mut rects: Vec<Rect>, w: u32) -> Vec<Rect> {
    if c.entity.as_str() != "IBAN_CODE" || crate::detect::validate::iban(value) {
        return rects;
    }
    let read = crate::detect::validate::compact(value);
    let lost = iban_length(&read[..read.len().min(2)]).saturating_sub(read.len());
    let (Some(first), Some(last)) = (
        rects.iter().map(|r| r.x).min(),
        rects.iter_mut().max_by_key(|r| r.right()),
    ) else {
        return rects;
    };
    let per_char = (last.right() - first) as f32 / value.chars().count().max(1) as f32;
    let more = ((lost + lost / 4 + 1) as f32 * per_char).ceil() as u32;
    last.w = (last.right() + more).min(w) - last.x;
    rects
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
    // The page of the first readings: a line read again as handwriting
    // stands in its box beside it, and is read as a line of its own below.
    let first: Vec<Line> = lines.iter().filter(|l| !l.handwriting).cloned().collect();
    let p = page(&first);
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
            let rects = with_rest(&c, &text[c.start..c.end], rects, w);
            out.push((c, rects));
        }
    }
    let heads = heads(lines);
    // The lines that end in a person's name: its surname may stand at the
    // start of the line below.
    let mut ends_in_name = vec![false; lines.len()];
    for (i, line) in lines.iter().enumerate() {
        let one = Page {
            text: line.text.clone(),
            chars: line.chars.clone(),
        };
        let context = neighbours(lines, &heads, i);
        for text in std::iter::once(line.text.clone()).chain(unmarked(&line.text)) {
            for mut c in read(&text, &context)? {
                if c.entity.as_str() == "PERSON"
                    && text == line.text
                    && !text[c.end..].chars().any(char::is_alphanumeric)
                {
                    ends_in_name[i] = true;
                }
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
                let rects = with_rest(&c, &text[c.start..c.end], rects, w);
                out.push((c, rects));
            }
        }
    }
    // A line and the one right below it that starts with a number, read as
    // two lines: a letter's address block (`Am Anger 7` / `15938 Lindau`),
    // which a row of the page cuts where a window beside it holds lines of
    // its own, and a line read alone does not hold whole. So too a line that
    // ends in a name and the line under it in its column that starts with a
    // capital: a headline broken after a given name (`… noch Jonas` /
    // `Kellermann!: …`), whose lines stand further apart than a cell's.
    for (i, line) in lines.iter().enumerate() {
        let next = lines.iter().filter(|l| {
            (l.text.starts_with(|c: char| c.is_ascii_digit()) && below(line, l))
                || (ends_in_name[i] && l.text.starts_with(char::is_uppercase) && under(line, l))
        });
        for l in next {
            let base = line.text.len() + 1;
            let two = Page {
                text: format!("{}\n{}", line.text, l.text),
                chars: line
                    .chars
                    .iter()
                    .copied()
                    .chain(l.chars.iter().map(|(s, e, r)| (base + s, base + e, *r)))
                    .collect(),
            };
            for c in read(&two.text, &neighbours(lines, &heads, i))? {
                let rects = boxes(&two, c.start, c.end, w, h);
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
///
/// `regions` (in the band) are what the other detectors found in the same
/// unfilled picture (#108): a candidate inside one is theirs, a line read
/// badly inside one is no reason for review, and they are filled with the
/// first reading's boxes. The boxes returned are the reading's own.
#[allow(clippy::too_many_arguments)]
fn settle(
    engine: &Engine,
    ocr: &Ocr,
    img: &mut RgbaImage,
    frame: Option<u64>,
    apply: bool,
    out: &mut Outcome,
    band: Band,
    regions: &[Rect],
) -> Result<Option<Vec<Rect>>, Error> {
    let (w, h) = img.dimensions();
    let mut filled: Vec<Rect> = Vec::new();
    for round in 1..=ROUNDS {
        let before = out.findings.len();
        let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
        // A band's open edge is no edge of the picture: a line cut there is
        // read when the band grows, and looked for there it made the band
        // grow toward the whole frame (#129).
        let lines = ocr.read_edges(&rgb, !band.open_above, !band.open_below)?;
        if lines.iter().any(|l| {
            (band.open_above && l.rect.y <= EDGE)
                || (band.open_below && l.rect.bottom() + EDGE >= h)
        }) {
            return Ok(None);
        }
        // The machine's reading of handwriting is no transcript a person
        // confirmed (#59): the report says it was read so.
        if lines.iter().any(|l| l.handwriting)
            && !out.review_reasons.iter().any(|r| r == HANDWRITING)
        {
            out.review_reasons.push(HANDWRITING.into());
        }
        // A line the edge cuts that could not be read holds no text to
        // detect in; it is filled whole below (ADR-0059).
        let read: Vec<Line> = lines
            .iter()
            .filter(|l| !l.text.trim().is_empty())
            .cloned()
            .collect();
        let candidates = candidates(engine, &read, w, h, round > 1)?;
        let mut new: Vec<Rect> = Vec::new();
        for (c, rects) in candidates {
            let rects: Vec<Rect> = rects.into_iter().map(|r| to_ink(&rgb, r)).collect();
            // Found twice (page and line), inside a box already filled, or
            // inside a region.
            if rects.is_empty()
                || rects
                    .iter()
                    .all(|r| covered(r, filled.iter().chain(regions).chain(&new)))
            {
                continue;
            }
            // In an area the user released (#34), in every round: a check
            // must not find it again.
            if rects.iter().all(|r| {
                released(
                    engine,
                    frame,
                    &Rect {
                        y: r.y + band.top,
                        ..*r
                    },
                )
            }) {
                continue;
            }
            if matches!(
                engine.action_for(&c),
                crate::Action::Pseudonymize | crate::Action::Fake
            ) && !out.review_reasons.iter().any(|r| r == BOXES_ONLY)
            {
                out.review_reasons.push(BOXES_ONLY.into());
            }
            // A finding per box filled: a candidate read across lines (a
            // tab's label in a sidebar and a word at the page's far side)
            // fills its words, and one box around them all would report a
            // band across the picture that was never filled.
            for r in rects
                .iter()
                .filter(|r| !covered(r, filled.iter().chain(regions).chain(&new)))
            {
                out.findings.push(Finding {
                    entity: c.entity.clone(),
                    location: Location::Region {
                        part: None,
                        frame,
                        x: r.x,
                        y: r.y + band.top,
                        width: r.w,
                        height: r.h,
                    },
                    confidence: c.confidence,
                    detector: c.detector.clone(),
                    pseudonym: None,
                });
            }
            new.extend(rects);
        }
        // Read badly where nothing is filled: on the first reading only, as
        // a later one reads the boxes' edges.
        if round == 1 {
            let quiet: Vec<Rect> = new.iter().chain(regions).copied().collect();
            let unread = unread(engine, &lines, &quiet, &band, frame, (w, h), out)?;
            new.extend(unread);
        }
        // The regions are filled with the first reading's boxes, and the
        // picture is read again to verify it.
        let regions_open = round == 1 && !regions.is_empty();
        if (new.is_empty() && !regions_open) || !apply {
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
        for r in new.iter().chain(if round == 1 { regions } else { &[] }) {
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

/// How far a pixel's channels lie from the background's for it to be ink.
const INK: u8 = 60;

/// `r` grown up and down while its top or bottom row cuts ink: a hand's
/// loops and tails reach past the line's box that OCR found, and the rows
/// of them left unfilled showed part of the value (#59). Printed text has
/// background at its box's edges and keeps its box. At most half the box's
/// height each way, and never past an empty row.
fn to_ink(img: &image::RgbImage, r: Rect) -> Rect {
    let (w, h) = img.dimensions();
    let (x0, x1) = (r.x.min(w), r.right().min(w));
    if x0 >= x1 || r.y >= h || r.h == 0 {
        return r;
    }
    // The background: the colour most pixels of the box have.
    let mut counts: std::collections::HashMap<[u8; 3], usize> = std::collections::HashMap::new();
    for y in r.y..r.bottom().min(h) {
        for x in x0..x1 {
            *counts.entry(img.get_pixel(x, y).0).or_default() += 1;
        }
    }
    let Some(bg) = counts
        .into_iter()
        .max_by_key(|(c, n)| (*n, *c))
        .map(|(c, _)| c)
    else {
        return r;
    };
    let ink = |y: u32| {
        (x0..x1).any(|x| {
            img.get_pixel(x, y)
                .0
                .iter()
                .zip(bg)
                .any(|(p, b)| p.abs_diff(b) > INK)
        })
    };
    let limit = r.h / 2;
    let mut top = r.y;
    while top > 0 && r.y - top < limit && ink(top) {
        top -= 1;
    }
    let mut bottom = r.bottom().min(h);
    while bottom < h && bottom - r.bottom().min(h) < limit && ink(bottom - 1) {
        bottom += 1;
    }
    Rect {
        y: top,
        h: bottom - top,
        ..r
    }
}

/// The lines OCR read badly where nothing in `new` covers them, as the
/// policy says (ADR-0045): `review` notes each for the report's reason,
/// `box` reports each as `UNREAD_TEXT` and returns its box to fill, `error`
/// stops at the first. A line the picture's edge cuts, read badly or not at
/// all, is filled whole whatever the policy says (ADR-0059): what is left of
/// it cannot be read better.
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
    // A reading as handwriting is of a line read badly already: that line
    // stands for both.
    for l in lines.iter().filter(|l| !l.handwriting) {
        let weak = l.weak().any(|r| !covered(r, new.iter()));
        let cut = l.cut && (weak || (l.text.trim().is_empty() && !covered(&l.rect, new.iter())));
        if !weak && !cut {
            continue;
        }
        let r = Rect {
            y: l.rect.y + band.top,
            ..l.rect
        };
        // A line in an area the user released (#34) is read as it is.
        if released(engine, frame, &r) {
            continue;
        }
        let low = if cut { LowConfidence::Box } else { low };
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
                    detector: if cut { CUT_DETECTOR } else { UNREAD_DETECTOR }.into(),
                    pseudonym: None,
                });
                boxes.push(grown(&l.rect, w, h));
            }
        }
    }
    Ok(boxes)
}

/// The part of a picture (`bytes`, any format veilq reads, or a PDF page
/// rendered as PNG) around `area`, `margin` pixels on each side within the
/// picture, as PNG: the excerpt of a case (#34).
pub fn crop(bytes: &[u8], area: &crate::Area, margin: u32) -> Result<Vec<u8>, Error> {
    let img = image::load_from_memory_with_format(bytes, format_of(bytes)?)
        .map_err(error)?
        .to_rgba8();
    let (w, h) = img.dimensions();
    let x = area.x.saturating_sub(margin).min(w);
    let y = area.y.saturating_sub(margin).min(h);
    let right = (area.x + area.width + margin).min(w);
    let bottom = (area.y + area.height + margin).min(h);
    if right <= x || bottom <= y {
        return Err(error("the area lies outside the picture"));
    }
    let part = image::imageops::crop_imm(&img, x, y, right - x, bottom - y).to_image();
    encode(&part, ImageFormat::Png)
}

/// A GIF's decoder with the limits a still picture has ([`image::Limits`]'
/// default, 512 MiB per allocation): every frame is decoded on the whole
/// screen a GIF declares, and a 35-byte file can declare 65535 x 65535,
/// some 17 GB.
fn gif_decoder(bytes: &[u8]) -> Result<GifDecoder<Cursor<&[u8]>>, Error> {
    let mut decoder = GifDecoder::new(Cursor::new(bytes)).map_err(error)?;
    decoder
        .set_limits(image::Limits::default())
        .map_err(error)?;
    let (w, h) = decoder.dimensions();
    if u64::from(w) * u64::from(h) * 4 > image::Limits::default().max_alloc.unwrap_or(u64::MAX) {
        return Err(error(format!(
            "a GIF of {w} x {h} pixels is too large to read"
        )));
    }
    Ok(decoder)
}

/// The pixels a GIF's frames may come to, each decoded on its whole screen:
/// some 10,000 frames at 1920 x 1080. The screen alone is bounded by
/// [`gif_decoder`]; a file of many tiny frames on a large screen took as
/// long as its frames times its screen (#138).
const MAX_GIF_PIXELS: u64 = 20_000_000_000;

/// A GIF's frames as they are decoded, an error once they come to more than
/// `max` pixels.
fn gif_frames(
    bytes: &[u8],
    max: u64,
) -> Result<impl Iterator<Item = Result<image::Frame, Error>> + '_, Error> {
    let decoder = gif_decoder(bytes)?;
    let (w, h) = decoder.dimensions();
    let screen = u64::from(w) * u64::from(h);
    Ok(decoder.into_frames().zip(1u64..).map(move |(f, n)| {
        if n.saturating_mul(screen) > max {
            return Err(error(format!(
                "a GIF of {w} x {h} pixels with more than {} frames is too large to read",
                n - 1
            )));
        }
        f.map_err(error)
    }))
}

/// Frame `n` (from 1) of a GIF as PNG, as decoded, and how many frames it
/// has: what an area is drawn on and cropped from (#114).
pub fn gif_frame(bytes: &[u8], n: u64) -> Result<(Vec<u8>, u64), Error> {
    let frames = gif_frames(bytes, MAX_GIF_PIXELS)?;
    let mut picked = None;
    let mut count = 0;
    for frame in frames {
        let frame = frame?;
        count += 1;
        if count == n {
            picked = Some(encode(&frame.into_buffer(), ImageFormat::Png)?);
        }
    }
    match picked {
        Some(png) => Ok((png, count)),
        None => Err(error(format!("the GIF has no frame {n} ({count} frames)"))),
    }
}

/// Frame `n` (from 1) of a GIF as PNG, as [`gif_frame`], the frames after
/// it not decoded: where their count is known already (#129).
pub fn gif_frame_at(bytes: &[u8], n: u64) -> Result<Vec<u8>, Error> {
    let mut count = 0;
    for frame in gif_frames(bytes, MAX_GIF_PIXELS)? {
        let frame = frame?;
        count += 1;
        if count == n {
            return encode(&frame.into_buffer(), ImageFormat::Png);
        }
    }
    Err(error(format!("the GIF has no frame {n} ({count} frames)")))
}

/// What OCR reads inside `area` of `picture` (a still image, or a GIF's
/// frame or a PDF's page as PNG): the line it stands in and the byte range
/// of the words inside the area, in the line that has the most of them
/// (#114). `None` when no word lies inside.
pub fn read_area(
    ocr: &Ocr,
    picture: &[u8],
    area: &crate::Area,
) -> Result<Option<(String, std::ops::Range<usize>)>, Error> {
    let mut decoder = ImageReader::with_format(Cursor::new(picture), format_of(picture)?)
        .into_decoder()
        .map_err(error)?;
    let orientation = decoder.orientation().map_err(error)?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(error)?;
    img.apply_orientation(orientation);
    let lines = ocr.read(&img.to_rgb8())?;
    Ok(inside(&lines, &rect(area)))
}

/// The line of `lines` with the most letters inside `r`, and the range of
/// its words inside: a word whose letters' centres lie in `r` at least half,
/// as a frame drawn by hand cuts a word's edge or touches the next one. A
/// word of no letter or digit at either end (`·`, `-`) is left out.
fn inside(lines: &[Line], r: &Rect) -> Option<(String, std::ops::Range<usize>)> {
    let within = |c: &Rect| {
        let (x, y) = (c.x + c.w / 2, c.y + c.h / 2);
        r.x <= x && x < r.right() && r.y <= y && y < r.bottom()
    };
    lines
        .iter()
        .filter_map(|l| {
            let is_in = |at: usize| {
                l.chars
                    .iter()
                    .any(|(s, e, c)| *s <= at && at < *e && within(c))
            };
            // The words, as byte ranges, with how many of their letters are in.
            let mut words = Vec::new();
            for (start, word) in l.text.split_whitespace().map(|w| {
                let at = w.as_ptr() as usize - l.text.as_ptr() as usize;
                (at, w)
            }) {
                let total = word.chars().count();
                let n = word
                    .char_indices()
                    .filter(|(i, _)| is_in(start + i))
                    .count();
                let wordy = word.chars().any(char::is_alphanumeric);
                if 2 * n >= total && n > 0 && wordy {
                    words.push((start..start + word.len(), n));
                }
            }
            let start = words.first()?.0.start;
            let end = words.last()?.0.end;
            let n: usize = words.iter().map(|(_, n)| n).sum();
            Some((n, (l.text.clone(), start..end)))
        })
        .max_by_key(|(n, _)| *n)
        .map(|(_, found)| found)
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
        removed_headers: 0,
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

/// The parts of `regions` (in a picture) between rows `top` and `bottom`, in
/// the band those rows make.
fn in_band(regions: &[Rect], top: u32, bottom: u32) -> Vec<Rect> {
    regions
        .iter()
        .filter_map(|r| {
            let y = r.y.max(top);
            let end = r.bottom().min(bottom);
            (end > y).then(|| Rect {
                y: y - top,
                h: end - y,
                ..*r
            })
        })
        .collect()
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
    // An area marked on this frame, filled with the bands' first reading's
    // boxes as on a whole picture (#114).
    let mut boxes = marked(engine, img, frame, out);
    boxes.extend(faces(engine, img, frame, out, kept)?);
    boxes.extend(signatures(engine, img, frame, out, kept)?);
    let margin = MARGIN.max(h / 20);
    let mut bands = changed_bands(before, after, margin);
    // Read each band on a copy; a band cut short starts the reading over, so
    // nothing of a reading given up reaches the frame or the report.
    let read = 'read: loop {
        if too_much(&bands, h) {
            let read = read_whole(engine, ocr, img, frame, apply, out, &boxes)?;
            boxes.extend(read);
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
            let regions = in_band(&boxes, top, bottom);
            match settle(
                engine, ocr, &mut part, frame, apply, &mut found, band, &regions,
            )? {
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
    // The faces and signatures whole, where a band holds a part of one only.
    if apply {
        for r in &boxes {
            fill(img, r);
        }
    }
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
    let frames = gif_frames(bytes, MAX_GIF_PIXELS)?;
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
        let frame = frame?;
        let delay = frame.delay();
        let original = frame.into_buffer();
        let n = Some(i as u64 + 1);
        let img = match &prev {
            // A frame like the one before keeps its boxes; an area marked on
            // it is filled from here on, as a box found here would be (#114).
            Some((before, done)) if *before == original => {
                let mut img = done.clone();
                let areas = marked(engine, &original, n, &mut out);
                if apply {
                    for r in &areas {
                        fill(&mut img, r);
                    }
                }
                boxes.extend(areas);
                img
            }
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
            // The mark goes onto the written frame only: the next frame is
            // compared with this one as redacted, unmarked.
            let mut written = img.clone();
            if engine.watermarks() {
                let (w, h) = written.dimensions();
                let scale = crate::watermark::scale_for(w, h);
                crate::watermark::stamp(&mut written, scale, scale * 2);
            }
            e.encode_frame(Frame::from_parts(written, 0, 0, delay))
                .map_err(error)?;
        }
        prev = Some((original, img));
    }
    drop(encoder);
    out.output = buf;
    weak_reason(&mut out);
    Ok(out)
}

/// Whether a PNG has a text chunk (`tEXt`, `zTXt`, `iTXt`) or a JPEG a
/// comment (`COM`): text a decoder does not hand over.
fn text_chunks(bytes: &[u8], format: ImageFormat) -> bool {
    let be = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
    match format {
        ImageFormat::Png => {
            let mut i = 8;
            while i + 8 <= bytes.len() {
                let len = be(&bytes[i..]);
                if matches!(&bytes[i + 4..i + 8], b"tEXt" | b"zTXt" | b"iTXt") {
                    return true;
                }
                i += 12 + len;
            }
            false
        }
        ImageFormat::Jpeg => {
            let mut i = 2;
            while i + 4 <= bytes.len() && bytes[i] == 0xFF {
                let marker = bytes[i + 1];
                // The scan's data starts: no more segments.
                if marker == 0xDA {
                    return false;
                }
                if marker == 0xFE {
                    return true;
                }
                i += 2 + usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
            }
            false
        }
        _ => false,
    }
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
    // What the encoding drops is reported (ADR-0063): EXIF, XMP, IPTC, a
    // PNG's text chunks, a JPEG's comments.
    let held =
        |m: image::ImageResult<Option<Vec<u8>>>| m.ok().flatten().is_some_and(|m| !m.is_empty());
    let metadata = held(decoder.exif_metadata())
        || held(decoder.xmp_metadata())
        || held(decoder.iptc_metadata())
        || text_chunks(bytes, format);
    let mut img = DynamicImage::from_decoder(decoder).map_err(error)?;
    img.apply_orientation(orientation);
    let mut rgba = img.to_rgba8();
    let mut out = outcome();
    if metadata {
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
        // After the verify rounds: OCR never reads the mark (ADR-0050).
        if engine.watermarks() {
            let scale = crate::watermark::scale_for(rgba.width(), rgba.height());
            crate::watermark::stamp(&mut rgba, scale, scale * 2);
        }
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
            cut: false,
            handwriting: false,
        }
    }

    fn area(frame: Option<u64>, x: u32, y: u32, width: u32, height: u32) -> crate::Area {
        crate::Area {
            frame,
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_marked_area_is_reported_on_its_page_only_and_left_to_fill() {
        let engine = Engine::new(&crate::Policy::default(), None, b"test", None)
            .unwrap()
            .with_picture_marks(
                vec![
                    (
                        area(Some(2), 10, 10, 20, 10),
                        EntityType::new("PERSON").unwrap(),
                    ),
                    // Beyond the picture's edge: clipped to it.
                    (
                        area(Some(2), 90, 90, 50, 50),
                        EntityType::new("ADDRESS").unwrap(),
                    ),
                ],
                Vec::new(),
            );
        let white = RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255]));
        let mut out = outcome();
        assert!(
            marked(&engine, &white, Some(1), &mut out).is_empty(),
            "another page"
        );
        assert!(out.findings.is_empty());
        // Filled with the first reading's boxes, not before it (#108).
        let boxes = marked(&engine, &white, Some(2), &mut out);
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[1].w, 10, "clipped at the edge");
        assert_eq!(out.findings.len(), 2);
        assert_eq!(out.findings[0].detector, MARK_DETECTOR);
        assert_eq!(out.findings[0].confidence, 1.0);
    }

    #[test]
    fn a_region_is_cut_to_the_band_it_reaches_into() {
        let face = Rect {
            x: 5,
            y: 40,
            w: 20,
            h: 30,
        };
        assert_eq!(
            in_band(&[face], 50, 100),
            vec![Rect {
                x: 5,
                y: 0,
                w: 20,
                h: 20
            }]
        );
        assert_eq!(in_band(&[face], 0, 50)[0].h, 10);
        assert!(in_band(&[face], 70, 90).is_empty(), "below it");
    }

    #[test]
    fn what_lies_in_a_released_area_is_left() {
        let engine = Engine::new(&crate::Policy::default(), None, b"test", None)
            .unwrap()
            .with_picture_marks(Vec::new(), vec![area(None, 0, 0, 50, 50)]);
        let inside = Rect {
            x: 10,
            y: 10,
            w: 20,
            h: 10,
        };
        let across = Rect {
            x: 40,
            y: 40,
            w: 30,
            h: 10,
        };
        assert!(released(&engine, None, &inside));
        assert!(!released(&engine, None, &across), "most of it outside");
        assert!(!released(&engine, Some(1), &inside), "another page");
        // A face found there is not reported or filled.
        let region = Region {
            entity: "FACE",
            detector: "face",
            reason: FACES_FOUND,
        };
        let mut out = outcome();
        let filled = regions(
            &engine,
            &region,
            vec![(inside, 0.99), (across, 0.99)],
            None,
            &mut out,
            &[],
        );
        assert_eq!(filled, vec![across]);
        assert_eq!(out.findings.len(), 1);
    }

    #[test]
    fn a_case_excerpt_is_the_picture_around_the_area() {
        let img = RgbaImage::from_pixel(100, 80, Rgba([255, 255, 255, 255]));
        let png = encode(&img, ImageFormat::Png).unwrap();
        let part = crop(&png, &area(None, 10, 10, 20, 10), 5).unwrap();
        let part = image::load_from_memory(&part).unwrap();
        assert_eq!((part.width(), part.height()), (30, 20));
        // At the edge, the margin ends with the picture.
        let edge =
            image::load_from_memory(&crop(&png, &area(None, 95, 75, 20, 20), 5).unwrap()).unwrap();
        assert_eq!((edge.width(), edge.height()), (10, 10));
        assert!(crop(&png, &area(None, 200, 200, 5, 5), 0).is_err());
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
    fn a_line_read_badly_inside_a_region_is_no_reason() {
        let engine = Engine::new(&crate::Policy::default(), None, b"test", None).unwrap();
        let mut l = line("Lena Albrecht", 0, 0);
        l.scores[5] = 0.6;
        // A signature's box over the line (#108): filled whole, so nothing
        // read badly there can leave.
        let signature = Rect {
            x: 0,
            y: 0,
            w: 400,
            h: 40,
        };
        let mut out = outcome();
        let lines = [l];
        let boxes = unread(
            &engine,
            &lines,
            &[signature],
            &Band::WHOLE,
            None,
            (1000, 1000),
            &mut out,
        );
        assert!(boxes.unwrap().is_empty());
        assert!(out.weak_lines.is_empty());
        unread(
            &engine,
            &lines,
            &[],
            &Band::WHOLE,
            None,
            (1000, 1000),
            &mut out,
        )
        .unwrap();
        assert_eq!(out.weak_lines.len(), 1, "outside every region");
    }

    #[test]
    fn a_line_the_edge_cuts_is_filled_when_it_cannot_be_read() {
        // ADR-0059: with the default policy (`review`) too.
        let engine = Engine::new(&crate::Policy::default(), None, b"test", None).unwrap();
        let mut weak = line("Lina Petersen", 0, 990);
        weak.cut = true;
        weak.scores[6] = 0.5;
        let mut blank = line("", 300, 992);
        blank.rect = Rect {
            x: 300,
            y: 992,
            w: 120,
            h: 8,
        };
        blank.cut = true;
        let mut clear = line("Freundliche Grüße", 600, 990);
        clear.cut = true;
        let mut out = outcome();
        let boxes = unread(
            &engine,
            &[weak, blank, clear],
            &[],
            &Band::WHOLE,
            None,
            (1000, 1000),
            &mut out,
        )
        .unwrap();
        assert_eq!(boxes.len(), 2, "{boxes:?}");
        assert!(out.weak_lines.is_empty(), "filled, not left for review");
        assert!(
            out.findings
                .iter()
                .all(|f| f.detector == CUT_DETECTOR && f.entity.as_str() == UNREAD_TEXT)
        );
        // Read well, a cut line is read like any other.
        assert!(boxes.iter().all(|b| b.x < 600));
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
        // Its margin: a quarter of the line's 20 pixels to the sides, 2 up
        // and down.
        let b = boxes(&p, start, start + 10, 1000, 1000);
        assert_eq!(
            b,
            vec![Rect {
                x: 195,
                y: 0,
                w: 110,
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

    #[test]
    fn a_letters_address_block_beside_a_window_is_an_address() {
        // A letter's preview beside an application's window: the window's
        // lines stand in the rows of the letter's, and the page's rows cut
        // the address block into two cells.
        let engine = Engine::new(&crate::Policy::default(), None, b"test", None).unwrap();
        let lines = [
            line("Elemente", 0, 100),
            line("Auf der Höhe 9", 600, 100),
            line("Name  Rechnungsnummer", 0, 122),
            line("15938 Lindau", 600, 122),
        ];
        let found: Vec<Rect> = candidates(&engine, &lines, 2000, 300, false)
            .unwrap()
            .into_iter()
            .filter(|(c, _)| c.entity.as_str() == "ADDRESS")
            .flat_map(|(_, rects)| rects)
            .collect();
        // Both lines of the block, nothing of the window.
        assert!(found.iter().any(|r| r.y < 110 && r.x >= 590), "{found:?}");
        assert!(found.iter().any(|r| r.y >= 110 && r.x >= 590), "{found:?}");
        assert!(found.iter().all(|r| r.x >= 590), "{found:?}");
    }

    #[test]
    fn a_name_broken_after_its_given_name_is_read_with_the_line_below() {
        // A headline in a card beside others, broken after a given name;
        // its lines stand further apart than a cell's. A rule of the policy
        // that finds the whole name stands in for NER here.
        let policy = crate::Policy::parse(
            "version = 1\n[[patterns]]\nname = \"full-name\"\nentity = \"PERSON\"\nregex = 'Mira\\sKellermann'\n",
        )
        .unwrap();
        let engine = Engine::new(&policy, None, b"test", None).unwrap();
        let line = |text: &str, x: u32, y: u32| Line {
            rect: Rect {
                h: 17,
                ..super::tests::line(text, x, y).rect
            },
            ..super::tests::line(text, x, y)
        };
        let lines = [
            line("Neue Bruecke am Hafen", 20, 52),
            line("Termin mit Frau Mira", 350, 52),
            line("Stadtbote meldet", 690, 52),
            line("wird im Mai eroeffnet", 20, 81),
            line("Kellermann!: Wenn alle", 350, 81),
            line("nicht vergeben", 690, 81),
        ];
        let found: Vec<Rect> = candidates(&engine, &lines, 1000, 200, false)
            .unwrap()
            .into_iter()
            .filter(|(c, _)| c.entity.as_str() == "PERSON")
            .flat_map(|(_, rects)| rects)
            .collect();
        // The surname at the start of the line below, in the same card.
        assert!(
            found.iter().any(|r| r.y >= 75 && (340..400).contains(&r.x)),
            "{found:?}"
        );
        // Not a line that only stands below in another card.
        assert!(found.iter().all(|r| (340..700).contains(&r.x)), "{found:?}");
    }

    #[test]
    fn a_value_read_only_as_handwriting_is_filled() {
        // OCR made words of a hand-filled IBAN, past what a reading of OCR
        // forgives (ADR-0037); the reading as handwriting, beside it in the
        // same box, has it (#59).
        let engine = Engine::new(&crate::Policy::default(), None, b"test", None).unwrap();
        let misread = line("Dass Bezug oder Zahlweise", 200, 100);
        let hand = Line {
            handwriting: true,
            ..line("DE38 3253 0528 2447 0698 76", 200, 100)
        };
        let ibans = |lines: &[Line]| -> Vec<Rect> {
            candidates(&engine, lines, 2000, 300, false)
                .unwrap()
                .into_iter()
                .filter(|(c, _)| c.entity.as_str() == "IBAN_CODE")
                .flat_map(|(_, rects)| rects)
                .collect()
        };
        assert!(ibans(std::slice::from_ref(&misread)).is_empty());
        let found = ibans(&[misread.clone(), hand]);
        assert!(!found.is_empty());
        // In the line's box, padded.
        assert!(found.iter().all(|r| r.y >= 95 && r.x >= 195), "{found:?}");
    }

    #[test]
    fn a_box_that_cuts_ink_grows_until_the_ink_ends() {
        // A hand's tail two rows below the box, and a form's rule further
        // down, past an empty row (#59).
        let mut img = image::RgbImage::from_pixel(100, 60, image::Rgb([250, 250, 250]));
        for y in 20..32 {
            img.put_pixel(30, y, image::Rgb([20, 40, 130]));
        }
        for x in 0..100 {
            img.put_pixel(x, 40, image::Rgb([150, 150, 150]));
        }
        let r = Rect {
            x: 10,
            y: 18,
            w: 50,
            h: 12,
        };
        let grown = to_ink(&img, r);
        assert_eq!((grown.y, grown.bottom()), (18, 33));
        // Printed text with background at its edges keeps its box.
        let clear = Rect { y: 10, h: 8, ..r };
        assert_eq!(to_ink(&img, clear), clear);
    }

    #[test]
    fn an_iban_read_short_is_filled_as_long_as_it_is() {
        // Small text: OCR glued the label to the IBAN and lost five of its
        // 22 characters; the box reaches as far as they would.
        let engine = Engine::new(&crate::Policy::default(), None, b"test", None).unwrap();
        let iban = |lines: &[Line]| -> Vec<Rect> {
            candidates(&engine, lines, 2000, 300, false)
                .unwrap()
                .into_iter()
                .filter(|(c, _)| c.entity.as_str() == "IBAN_CODE")
                .flat_map(|(_, rects)| rects)
                .collect()
        };
        let short = [
            line("ANDE893704004405320", 100, 100),
            line("Status", 600, 100),
        ];
        let found = iban(&short);
        // Ten pixels a character: five lost, a space and one more past 290.
        assert!(found.iter().any(|r| r.right() >= 360), "{found:?}");
        assert!(found.iter().all(|r| r.right() < 600), "{found:?}");
        // An IBAN read whole keeps its own boxes.
        let whole = [
            line("DE89 3704 0044 0532 0130 00", 100, 100),
            line("BIC", 380, 100),
        ];
        let found = iban(&whole);
        assert!(!found.is_empty());
        assert!(found.iter().all(|r| r.right() < 380), "{found:?}");
        // A country with a longer IBAN than Germany's (#89): Portugal's 25
        // characters, four lost; 22 would widen by one.
        let portugal = [line("PT50 0002 0123 1234 5678 9", 100, 100)];
        let found = iban(&portugal);
        // Ten pixels a character: four lost, a space and one more past 360.
        assert!(found.iter().any(|r| r.right() >= 420), "{found:?}");
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
        // characters 0 to 7 (x 0 to 70), `Kapuze` starts at x 80; the box's
        // margin is a quarter of the line's 20 pixels.
        assert!(found.iter().any(|r| r.y < 20 && r.x == 0), "{found:?}");
        assert!(found.iter().any(|r| r.y >= 38), "{found:?}");
        assert!(
            found
                .iter()
                .filter(|r| r.y >= 38)
                .all(|r| r.right() <= 70 + 20 / 4),
            "{found:?}"
        );
    }

    #[test]
    fn a_name_in_a_tab_and_a_cut_sidebar_is_the_name_of_the_record() {
        // An application's record: the name in its header with a cue, the
        // same name in a tab cut short and in a sidebar the window cuts on
        // the left (ADR-0047).
        let engine = Engine::new(&crate::Policy::default(), None, b"test", None)
            .unwrap()
            .with_names();
        let lines = [
            line("Brandtmeier, Jon..", 600, 0),
            line("Herr Jonas Brandtmeier", 200, 60),
            line("andtmeier, Jonas", 0, 200),
        ];
        let found: Vec<Rect> = candidates(&engine, &lines, 2000, 300, false)
            .unwrap()
            .into_iter()
            .filter(|(c, _)| c.entity.as_str() == "PERSON")
            .flat_map(|(_, rects)| rects)
            .collect();
        for (y, x) in [(0, 600), (200, 0)] {
            assert!(
                found
                    .iter()
                    .any(|r| r.y + PAD >= y && r.y <= y && r.x <= x + PAD),
                "line at {y}: {found:?}"
            );
        }
        // The tab's cut given name too: `Jon` starts at x 730.
        assert!(
            found.iter().any(|r| r.y <= PAD && r.right() >= 750),
            "{found:?}"
        );
    }

    #[test]
    fn a_tab_cut_to_its_surname_is_a_name_in_a_legal_file() {
        // The surname stands nowhere else on the page (ADR-0051).
        let lines = [
            line("Übersicht", 0, 0),
            line("Albers,...", 200, 0),
            line("Termin am Montag", 0, 60),
        ];
        let people = |toml: &str| -> Vec<Rect> {
            let policy = crate::Policy::parse(&format!("version = 1\n{toml}")).unwrap();
            let engine = Engine::new(&policy, None, b"test", None).unwrap();
            candidates(&engine, &lines, 1000, 200, false)
                .unwrap()
                .into_iter()
                .filter(|(c, _)| c.entity.as_str() == "PERSON")
                .flat_map(|(_, rects)| rects)
                .collect()
        };
        let found = people("purposes = [\"legal\"]");
        assert!(
            found
                .iter()
                .any(|r| r.x <= 200 && r.right() >= 270 && r.y == 0),
            "{found:?}"
        );
        // Below the default threshold.
        assert!(people("").is_empty());
    }

    #[test]
    fn a_box_grows_with_its_line() {
        let r = |x, y, w, h| Rect { x, y, w, h };
        // Large or bold text: 10 pixels to the sides, 5 up and down.
        assert_eq!(grown(&r(100, 100, 300, 40), 1000, 1000), r(90, 95, 320, 50));
        // Small text keeps the least margin.
        assert_eq!(grown(&r(100, 100, 60, 8), 1000, 1000), r(98, 98, 64, 12));
        // Never beyond the picture.
        assert_eq!(grown(&r(2, 0, 60, 40), 70, 42), r(0, 0, 70, 42));
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

    /// A GIF of `colours.len()` frames of 8x8 pixels, each one colour.
    fn gif_of(colours: &[[u8; 3]]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut e = GifEncoder::new(&mut buf);
            for c in colours {
                let img = RgbaImage::from_pixel(8, 8, Rgba([c[0], c[1], c[2], 255]));
                e.encode_frame(Frame::new(img)).unwrap();
            }
        }
        buf
    }

    #[test]
    fn a_gif_frame_is_given_as_png_with_the_number_of_frames() {
        let gif = gif_of(&[[255, 255, 255], [0, 0, 0], [0, 0, 0]]);
        let (png, n) = gif_frame(&gif, 2).unwrap();
        assert_eq!(n, 3);
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(4, 4).0, [0, 0, 0, 255]);
        let (first, _) = gif_frame(&gif, 1).unwrap();
        let first = image::load_from_memory(&first).unwrap().to_rgba8();
        assert_eq!(first.get_pixel(4, 4).0, [255, 255, 255, 255]);
        for n in [0, 4] {
            let e = gif_frame(&gif, n).unwrap_err().to_string();
            assert!(e.contains("3 frames"), "{e}");
        }
        // Without counting the frames after it, the same frame.
        assert_eq!(gif_frame_at(&gif, 2).unwrap(), png);
        assert!(gif_frame_at(&gif, 4).is_err());
    }

    #[test]
    fn a_gif_of_more_frames_than_its_screen_allows_is_an_error() {
        // Each frame is decoded on the whole screen: 64 pixels here, the
        // bound in the run 20 Gpx (#138).
        let gif = gif_of(&[[0, 0, 0]; 5]);
        let read = |max| gif_frames(&gif, max).and_then(|f| f.collect::<Result<Vec<_>, _>>());
        assert_eq!(read(5 * 64).map(|f| f.len()).ok(), Some(5));
        let Err(e) = read(4 * 64) else {
            panic!("five frames read within four frames' pixels")
        };
        let e = e.to_string();
        assert!(e.contains("8 x 8") && e.contains("4 frames"), "{e}");
    }

    #[test]
    fn a_gif_that_declares_a_huge_screen_is_an_error_not_17_gb() {
        // A one-frame GIF of 8 x 8 whose screen says 65535 x 65535: every
        // frame would be decoded on that screen.
        let mut gif = gif_of(&[[0, 0, 0]]);
        gif[6..10].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        let e = gif_frame(&gif, 1).unwrap_err().to_string();
        assert!(e.contains("too large") || e.contains("limit"), "{e}");
    }

    #[test]
    fn an_area_reads_the_words_inside_it_in_the_line_with_most_of_them() {
        // Ten pixels per letter: `KD-48213` from x 140 to 220.
        let lines = [
            line("Kundennummer KD-48213", 10, 0),
            line("Raum KG-30551", 10, 40),
        ];
        let area = |x, y, w, h| Rect { x, y, w, h };
        let (text, r) = inside(&lines, &area(136, 2, 90, 16)).unwrap();
        assert_eq!(&text[r], "KD-48213");
        // A frame that cuts the word's first letters takes it whole, one that
        // touches the word before at one letter does not; the line with most
        // letters wins.
        let (text, r) = inside(&lines, &area(160, 0, 100, 60)).unwrap();
        assert_eq!(&text[r], "KD-48213");
        let (text, r) = inside(&lines, &area(120, 0, 105, 60)).unwrap();
        assert_eq!(&text[r], "KD-48213");
        // A dot between words is no word.
        let dotted = [line("Vorgang 2026-9081-4214 · Version 1.6", 0, 0)];
        let (text, r) = inside(&dotted, &area(85, 0, 160, 20)).unwrap();
        assert_eq!(&text[r], "2026-9081-4214");
        assert!(
            inside(&lines, &area(400, 0, 50, 50)).is_none(),
            "nothing read there"
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
    fn a_pngs_text_chunk_and_a_jpegs_comment_are_metadata() {
        let img = RgbaImage::from_pixel(4, 4, Rgba([200, 10, 10, 255]));
        let png = encode(&img, ImageFormat::Png).unwrap();
        let jpeg = encode(&img, ImageFormat::Jpeg).unwrap();
        assert!(!text_chunks(&png, ImageFormat::Png));
        assert!(!text_chunks(&jpeg, ImageFormat::Jpeg));
        // After the IHDR chunk (8 + 25 bytes): an author.
        let text = b"Author\0Kai Lindqvist";
        let mut chunk = (text.len() as u32).to_be_bytes().to_vec();
        chunk.extend_from_slice(b"tEXt");
        chunk.extend_from_slice(text);
        chunk.extend_from_slice(&[0; 4]);
        let mut with = png[..33].to_vec();
        with.extend_from_slice(&chunk);
        with.extend_from_slice(&png[33..]);
        assert!(text_chunks(&with, ImageFormat::Png));
        let mut with = jpeg[..2].to_vec();
        with.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x05, b'K', b'a', b'i']);
        with.extend_from_slice(&jpeg[2..]);
        assert!(text_chunks(&with, ImageFormat::Jpeg));
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
        let filled = [grown(&lines[2].rect, 400, 200)];
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
            assert_eq!(r.unwrap(), [grown(&lines[1].rect, 400, 200)], "{toml}");
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
