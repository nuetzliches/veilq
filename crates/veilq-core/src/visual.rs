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
//! The output is encoded anew, so EXIF, XMP and text chunks are gone; when
//! the input had EXIF (camera, time, often GPS), the report has a `METADATA`
//! finding for it. EXIF orientation is applied first, so boxes and pixels
//! agree.

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Rgba, RgbaImage};

use crate::Engine;
use crate::Error;
use crate::finding::{EntityType, Finding, Location};
use crate::ocr::{Line, Ocr, Rect};
use crate::office::Outcome;

const PAD: u32 = 2;
const ROUNDS: usize = 3;

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
            if rects.is_empty()
                || rects
                    .iter()
                    .all(|r| filled.iter().chain(&new).any(|f| f.intersects(r)))
            {
                continue;
            }
            let bound = rects.iter().skip(1).fold(rects[0], |a, r| a.union(r));
            out.findings.push(Finding {
                entity: c.entity.clone(),
                location: Location::Region {
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
        if round == ROUNDS {
            return Err(Error::Leftover {
                entity: out
                    .findings
                    .last()
                    .map(|f| f.entity.as_str().to_owned())
                    .unwrap_or_default(),
                detector: "ocr".into(),
            });
        }
        for r in &new {
            fill(img, r);
        }
        filled.extend(new);
    }
    Ok(filled)
}

/// The format of an image file, from its bytes.
fn format_of(bytes: &[u8]) -> Result<ImageFormat, Error> {
    image::guess_format(bytes).map_err(|_| error("not an image veilq reads (PNG, JPEG, WebP, GIF)"))
}

/// A PNG, JPEG or WebP image, redacted and encoded anew (no metadata).
pub fn image(engine: &Engine, ocr: &Ocr, bytes: &[u8], apply: bool) -> Result<Outcome, Error> {
    let format = format_of(bytes)?;
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
    let mut out = Outcome {
        output: Vec::new(),
        findings: Vec::new(),
        mapping: Vec::new(),
        review_reasons: Vec::new(),
    };
    if exif {
        out.findings.push(Finding {
            entity: EntityType::new("METADATA").expect("a valid type"),
            location: Location::Region {
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
    fn encoding_writes_pixels_only() {
        let img = RgbaImage::from_pixel(4, 4, Rgba([200, 10, 10, 255]));
        for f in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP] {
            let bytes = encode(&img, f).unwrap();
            assert_eq!(image::guess_format(&bytes).unwrap(), f);
            assert!(!bytes.windows(4).any(|w| w == b"Exif"));
        }
    }
}
