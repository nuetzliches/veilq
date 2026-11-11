//! Images and GIFs (ADR-0023): OCR reads the text, the detectors find
//! personal data in it, and the characters that hold it are filled black.
//!
//! The lines of an image are read in rows, top to bottom, left to right, as
//! one text (two spaces between lines of a row, a line break between rows),
//! so that a label (`Geburtsdatum`) is context for the value beside it. A
//! finding becomes the boxes of its characters, per line, two pixels wider.
//! Every action is a black box: a pseudonym or a stand-in cannot be written
//! into pixels faithfully. **verify** reads the filled image again; what it
//! still finds is filled too, for up to three rounds (ADR-0022).
//!
//! A GIF is read frame by frame. A box stays filled in the following frames
//! as long as its pixels do not change there, so a value that stands still
//! is not visible for a frame because OCR missed it once; every frame that
//! changed is read and verified on its own. Unchanged frames are not read
//! again.
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

const PAD: u32 = 2;

/// Said when a policy asks for `pseudonymize` or `fake` in a picture.
pub const BOXES_ONLY: &str = "in a picture every action is a black box: a pseudonym or a stand-in cannot be written into pixels faithfully";
/// Verify rounds at most: reading a filled picture again moves rows and
/// labels, so a round can find what the one before did not (ADR-0026).
const ROUNDS: usize = 6;

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
        } => {
            let at = format!("{width}x{height} at {x},{y}");
            match frame {
                Some(n) => format!("{at} (frame or page {n})"),
                None => at,
            }
        }
        _ => String::new(),
    }
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
    // Rows: lines whose vertical middles fall into each other's height.
    let mut rows: Vec<Vec<&Line>> = Vec::new();
    for l in lines {
        let best = rows
            .iter()
            .enumerate()
            .map(|(i, r)| (i, overlap_y(&r[0].rect, &l.rect)))
            .filter(|(_, o)| *o >= 0.5)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((i, _)) => rows[i].push(l),
            None => rows.push(vec![l]),
        }
    }
    let mut text = String::new();
    let mut chars = Vec::new();
    for (i, row) in rows.iter_mut().enumerate() {
        row.sort_by_key(|l| l.rect.x);
        if i > 0 {
            text.push('\n');
        }
        for (j, l) in row.iter().enumerate() {
            if j > 0 {
                text.push_str("  ");
            }
            let base = text.len();
            text.push_str(&l.text);
            chars.extend(l.chars.iter().map(|(s, e, r)| (base + s, base + e, *r)));
        }
    }
    Page { text, chars }
}

/// How much two boxes overlap vertically, as a share of the lower one.
fn overlap_y(a: &Rect, b: &Rect) -> f32 {
    let top = a.y.max(b.y);
    let bottom = a.bottom().min(b.bottom());
    bottom.saturating_sub(top) as f32 / a.h.min(b.h).max(1) as f32
}

/// What stands next to a line: the nearest line on its left in the same
/// row, and the nearest line above it that overlaps it horizontally. A form
/// puts the label there (`Geburtsdatum  14.03.1987`); in a two-column
/// layout the rows of the page do not always pair them.
fn neighbours(lines: &[Line], i: usize) -> String {
    let l = &lines[i].rect;
    let left = lines
        .iter()
        .enumerate()
        .filter(|(j, o)| *j != i && o.rect.right() <= l.x + 2 && overlap_y(&o.rect, l) >= 0.4)
        .min_by_key(|(_, o)| l.x.saturating_sub(o.rect.right()));
    let above = lines
        .iter()
        .enumerate()
        .filter(|(j, o)| {
            *j != i && o.rect.bottom() <= l.y + 2 && o.rect.x < l.right() && l.x < o.rect.right()
        })
        .min_by_key(|(_, o)| l.y.saturating_sub(o.rect.bottom()));
    [above, left]
        .into_iter()
        .flatten()
        .map(|(_, o)| o.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
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
    let (w, h) = img.dimensions();
    let mut filled: Vec<Rect> = Vec::new();
    for round in 1..=ROUNDS {
        let before = out.findings.len();
        let rgb = DynamicImage::ImageRgba8(img.clone()).to_rgb8();
        let lines = ocr.read(&rgb)?;
        let p = page(&lines);
        // The page as one text, and each line with its neighbours as context.
        let mut candidates: Vec<(crate::detect::Candidate, Vec<Rect>)> = Vec::new();
        for c in engine.findings(&p.text, "")? {
            let rects = boxes(&p, c.start, c.end, w, h);
            candidates.push((c, rects));
        }
        for (i, line) in lines.iter().enumerate() {
            let one = Page {
                text: line.text.clone(),
                chars: line.chars.clone(),
            };
            for c in engine.findings(&line.text, &neighbours(&lines, i))? {
                let rects = boxes(&one, c.start, c.end, w, h);
                candidates.push((c, rects));
            }
        }
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
                    y: bound.y,
                    width: bound.w,
                    height: bound.h,
                },
                confidence: c.confidence,
                detector: c.detector.clone(),
                pseudonym: None,
            });
            new.extend(rects);
        }
        if new.is_empty() || !apply {
            return Ok(filled);
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
    Ok(filled)
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
    }
}

/// Whether the pixels inside `r` differ between `a` and `b`.
fn changed(a: &RgbaImage, b: &RgbaImage, r: &Rect) -> bool {
    (r.y..r.bottom().min(a.height()))
        .any(|y| (r.x..r.right().min(a.width())).any(|x| a.get_pixel(x, y) != b.get_pixel(x, y)))
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
                let found =
                    redact_picture(engine, ocr, &mut img, Some(i as u64 + 1), apply, &mut out)?;
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
            chars,
            score: 1.0,
        }
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
    fn encoding_writes_pixels_only() {
        let img = RgbaImage::from_pixel(4, 4, Rgba([200, 10, 10, 255]));
        for f in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            let bytes = encode(&img, f).unwrap();
            assert_eq!(image::guess_format(&bytes).unwrap(), f);
            assert!(!bytes.windows(4).any(|w| w == b"Exif"));
        }
    }
}
