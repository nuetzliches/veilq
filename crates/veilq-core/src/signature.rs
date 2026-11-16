//! Handwritten signatures in pictures (ADR-0048): YOLOS-tiny finds them,
//! veilq fills them black. It finds where a signature is; it never tells
//! whose it is (no features, no comparison), and nothing leaves the machine.
//!
//! The model reads 640 x 640 pixels (RGB, scaled to 0 to 1 and normalised
//! with ImageNet's mean and deviation), the picture resized to that square
//! whatever its shape. It gives 100 boxes (centre, width and height as
//! shares of what it read) with two logits each: signature and nothing. A
//! picture is read whole and, when much wider or taller than square (a
//! screenshot with a letter beside a window, an A4 page), also in squares of
//! its shorter side, overlapping by half: a signature on a page that fills a
//! third of the screen is read at a larger scale.

use std::path::Path;
use std::sync::Mutex;

use image::{RgbImage, imageops};
use ort::session::Session;
use ort::value::Tensor;

use crate::Error;
use crate::ocr::Rect;

/// The side the model reads.
const SIDE: u32 = 640;
/// A picture this much longer than wide (or the other way) is also read in
/// squares.
const SQUARES_FROM: f32 = 1.3;
/// The detector's own threshold: what the measurements of ADR-0048 found
/// all signatures at, but a watermark too (#80); a box filled black costs
/// less than a signature left.
pub const MIN_SCORE: f32 = 0.5;
/// A box narrower than this (in pixels of the picture) holds no signature
/// anyone could read: the model's few small boxes lay on empty space or a
/// mouse pointer in a screen recording.
const MIN_WIDTH: f32 = 40.0;
/// A box with this share of its area inside another is that one's: the same
/// signature, read from the whole picture and from a square.
const INSIDE: f32 = 0.7;
/// A box over this many lines of printed text is no signature but a
/// watermark across a page (`ENTWURF`, `KOPIE`), which the model takes for
/// one (#80). A signature crosses one or two: the name or the role under it.
const PRINTED_LINES: usize = 3;
/// A line counts as under a box with this share of its height inside it,
/// and this share of its length or [`WORDS`] times its height.
const LINE_INSIDE: f32 = 0.5;
const WORDS: f32 = 3.0;
/// A box is grown by this share of its size on every side: a stroke's tail
/// may end outside it.
const MARGIN: f32 = 0.05;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

fn error(message: impl std::fmt::Display) -> Error {
    Error::Format(format!("signature detection: {message}"))
}

/// A signature: its box in the picture, grown by [`MARGIN`], and its score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Signature {
    pub rect: Rect,
    pub score: f32,
}

/// What [`Signatures::find`] found in a picture.
#[derive(Debug, Default)]
pub struct Found {
    /// The signatures, the best first.
    pub signatures: Vec<Signature>,
    /// The boxes left out because they lay over printed text.
    pub printed: usize,
}

/// The files of an installed signature detector.
pub struct ModelFiles<'a> {
    pub model: &'a Path,
    pub runtime: &'a Path,
}

pub struct Signatures {
    session: Mutex<Session>,
}

/// A box in pixels of the picture, before it is grown and cut.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Candidate {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    score: f32,
}

impl Signatures {
    pub fn load(files: &ModelFiles) -> Result<Self, Error> {
        let env = crate::runtime::env(files.runtime)?;
        let session = Session::builder(env)
            .map_err(error)?
            .commit_from_file(files.model)
            .map_err(|e| error(format!("{}: {e}", files.model.display())))?;
        Ok(Self {
            session: Mutex::new(session),
        })
    }

    /// The signatures in `img`, the best first, and how many boxes lay over
    /// printed text and were left out. `text` gives the boxes of the lines
    /// of printed text in `img`; it is called only when there is a box.
    pub fn find(
        &self,
        img: &RgbImage,
        text: &mut dyn FnMut() -> Result<Vec<Rect>, Error>,
    ) -> Result<Found, Error> {
        let (w, h) = img.dimensions();
        let mut boxes = Vec::new();
        for (x, y, side_w, side_h) in parts(w, h) {
            let part = imageops::crop_imm(img, x, y, side_w, side_h).to_image();
            for f in self.read(&part)? {
                boxes.push(Candidate {
                    x: x as f32 + f.x,
                    y: y as f32 + f.y,
                    ..f
                });
            }
        }
        // Narrow boxes go first: kept, one could take the place of the box
        // of the whole signature and then be dropped itself.
        boxes.retain(|f| f.w >= MIN_WIDTH);
        if boxes.is_empty() {
            return Ok(Found::default());
        }
        // Then the boxes over printed text: a watermark across a page, not
        // a signature; kept, one would take the place of the signature
        // inside it and black out the text around.
        let lines = text()?;
        let before = boxes.len();
        boxes.retain(|f| !printed(f, &lines));
        let printed = before - boxes.len();
        let signatures = distinct(boxes)
            .into_iter()
            .filter_map(|f| {
                place(&f, w, h).map(|rect| Signature {
                    rect,
                    score: f.score,
                })
            })
            .collect();
        Ok(Found {
            signatures,
            printed,
        })
    }

    /// The signatures in `part`, in its pixels.
    fn read(&self, part: &RgbImage) -> Result<Vec<Candidate>, Error> {
        let (w, h) = part.dimensions();
        let input = imageops::resize(part, SIDE, SIDE, imageops::FilterType::Triangle);
        let side = SIDE as usize;
        let plane = side * side;
        let mut data = vec![0f32; 3 * plane];
        for (x, y, p) in input.enumerate_pixels() {
            let i = y as usize * side + x as usize;
            for c in 0..3 {
                data[c * plane + i] = (f32::from(p[c]) / 255.0 - MEAN[c]) / STD[c];
            }
        }
        let input = Tensor::from_array(([1usize, 3, side, side], data)).map_err(error)?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| error("a poisoned session"))?;
        let outputs = session.run(ort::inputs![input]).map_err(error)?;
        let tensor = |name: &str| {
            outputs
                .get(name)
                .ok_or_else(|| error(format!("the model has no output `{name}`")))?
                .try_extract_tensor::<f32>()
                .map(|(_, v)| v.to_vec())
                .map_err(error)
        };
        Ok(decode(
            &tensor("logits")?,
            &tensor("pred_boxes")?,
            w as f32,
            h as f32,
        ))
    }
}

/// The parts of a `w` x `h` picture that are read: the whole, and when it
/// is far from square, squares of its shorter side that overlap by half,
/// the last of them at the far edge.
fn parts(w: u32, h: u32) -> Vec<(u32, u32, u32, u32)> {
    let mut out = vec![(0, 0, w, h)];
    let (short, long) = (w.min(h), w.max(h));
    if short == 0 || (long as f32) < short as f32 * SQUARES_FROM {
        return out;
    }
    let step = (short / 2).max(1);
    let mut starts: Vec<u32> = (0..)
        .map(|i| i * step)
        .take_while(|s| s + short < long)
        .collect();
    starts.push(long - short);
    for s in starts {
        out.push(if w > h {
            (s, 0, short, short)
        } else {
            (0, s, short, short)
        });
    }
    out
}

/// The boxes of one reading, in pixels of a part `w` x `h`: per query two
/// logits (signature, nothing), a softmax, and the box as shares.
fn decode(logits: &[f32], boxes: &[f32], w: f32, h: f32) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (l, b) in logits
        .as_chunks::<2>()
        .0
        .iter()
        .zip(boxes.as_chunks::<4>().0)
    {
        let m = l[0].max(l[1]);
        let (a, n) = ((l[0] - m).exp(), (l[1] - m).exp());
        let score = a / (a + n);
        if score < MIN_SCORE {
            continue;
        }
        let (bw, bh) = (b[2] * w, b[3] * h);
        out.push(Candidate {
            x: b[0] * w - bw / 2.0,
            y: b[1] * h - bh / 2.0,
            w: bw,
            h: bh,
            score,
        });
    }
    out
}

/// The boxes to fill, the best first. A box that lies (nearly) inside
/// another is that one's: the outer box stays, with the better score, so a
/// box of part of the strokes never takes the place of the whole. Boxes
/// that only overlap both stay: filling both costs nothing, and joining them
/// would grow a box over what lies between (a watermark beside a signature).
fn distinct(mut found: Vec<Candidate>) -> Vec<Candidate> {
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut out: Vec<Candidate> = Vec::new();
    for f in found {
        if out.iter().any(|k| inside(&f, k)) {
            continue;
        }
        let score = out
            .iter()
            .filter(|k| inside(k, &f))
            .map(|k| k.score)
            .fold(f.score, f32::max);
        out.retain(|k| !inside(k, &f));
        out.push(Candidate { score, ..f });
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// Whether `f` lies over [`PRINTED_LINES`] lines of printed text or more. A
/// line is under it when half its height is, and half its length or a piece
/// as long as three times its height (a few words of a long line).
fn printed(f: &Candidate, lines: &[Rect]) -> bool {
    lines
        .iter()
        .filter(|l| {
            let w = (f.x + f.w).min((l.x + l.w) as f32) - f.x.max(l.x as f32);
            let h = (f.y + f.h).min((l.y + l.h) as f32) - f.y.max(l.y as f32);
            let (lw, lh) = (l.w as f32, l.h as f32);
            h >= LINE_INSIDE * lh && w >= (LINE_INSIDE * lw).min(WORDS * lh)
        })
        .count()
        >= PRINTED_LINES
}

/// Whether `a` lies (nearly) inside `b`.
fn inside(a: &Candidate, b: &Candidate) -> bool {
    let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    w > 0.0 && h > 0.0 && w * h >= INSIDE * a.w * a.h
}

/// `f` grown by [`MARGIN`] and cut to the picture; `None` outside it.
fn place(f: &Candidate, width: u32, height: u32) -> Option<Rect> {
    let (mx, my) = (f.w * MARGIN, f.h * MARGIN);
    let x0 = (f.x - mx).max(0.0);
    let y0 = (f.y - my).max(0.0);
    let x1 = (f.x + f.w + mx).ceil().min(width as f32);
    let y1 = (f.y + f.h + my).ceil().min(height as f32);
    (x1 > x0 && y1 > y0).then(|| Rect {
        x: x0 as u32,
        y: y0 as u32,
        w: (x1 - x0.floor()) as u32,
        h: (y1 - y0.floor()) as u32,
    })
}

/// A signature's score as a confidence the default threshold (0.7) lets
/// through: [`MIN_SCORE`] -> 0.7, 1.0 -> 1.0. A policy raises it for
/// `SIGNATURE` to fill fewer.
pub fn confidence(score: f32) -> f32 {
    let c = 0.7 + 0.3 * (score - MIN_SCORE) / (1.0 - MIN_SCORE);
    (c.clamp(0.0, 1.0) * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_read_whole_and_in_squares_when_far_from_square() {
        assert_eq!(parts(1000, 1200), vec![(0, 0, 1000, 1200)]);
        // An A4 page: whole, then its upper and lower square.
        assert_eq!(
            parts(1240, 1754),
            vec![(0, 0, 1240, 1754), (0, 0, 1240, 1240), (0, 514, 1240, 1240)]
        );
        // 2400 x 1200: whole, then squares at 0, 600 and 1200.
        assert_eq!(
            parts(2400, 1200),
            vec![
                (0, 0, 2400, 1200),
                (0, 0, 1200, 1200),
                (600, 0, 1200, 1200),
                (1200, 0, 1200, 1200),
            ]
        );
        // The last square ends at the far edge.
        let p = parts(2427, 1351);
        let last = p.last().unwrap();
        assert_eq!(last.0 + last.2, 2427);
        // Tall: squares down the page.
        assert_eq!(
            parts(500, 1000)[1..],
            [(0, 0, 500, 500), (0, 250, 500, 500), (0, 500, 500, 500)]
        );
    }

    #[test]
    fn a_query_becomes_a_box_in_the_part() {
        // Two queries: a signature (logits 2 and 0) in the middle, and
        // nothing (0 and 3).
        let logits = [2.0, 0.0, 0.0, 3.0];
        let boxes = [0.5, 0.25, 0.2, 0.1, 0.5, 0.5, 1.0, 1.0];
        let found = decode(&logits, &boxes, 1000.0, 800.0);
        assert_eq!(found.len(), 1);
        let f = found[0];
        assert!((f.score - 0.881).abs() < 1e-3, "{f:?}");
        assert!(
            (f.x - 400.0).abs() < 1e-3 && (f.y - 160.0).abs() < 1e-3,
            "{f:?}"
        );
        assert!(
            (f.w - 200.0).abs() < 1e-3 && (f.h - 80.0).abs() < 1e-3,
            "{f:?}"
        );
    }

    #[test]
    fn one_signature_found_twice_is_one() {
        let f = |x, y, w, h, score| Candidate { x, y, w, h, score };
        let found = distinct(vec![
            f(100.0, 100.0, 200.0, 60.0, 0.8),
            // The same from a square.
            f(104.0, 98.0, 196.0, 64.0, 0.93),
            // Another one beside it.
            f(600.0, 100.0, 200.0, 60.0, 0.7),
        ]);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].x, found[0].score), (104.0, 0.93));
        assert_eq!(found[1].x, 600.0);
        // Boxes that only overlap both stay, neither grown over the other.
        let found = distinct(vec![
            f(100.0, 100.0, 200.0, 100.0, 0.9),
            f(250.0, 150.0, 200.0, 100.0, 0.8),
        ]);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].w, found[1].w), (200.0, 200.0));
    }

    #[test]
    fn a_box_of_part_of_the_strokes_does_not_leave_the_rest() {
        let f = |x, y, w, h, score| Candidate { x, y, w, h, score };
        // A square's box over the left half scores higher than the box of
        // the whole signature: the whole stays covered.
        let found = distinct(vec![
            f(100.0, 100.0, 220.0, 60.0, 0.7),
            f(100.0, 100.0, 120.0, 60.0, 0.9),
        ]);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].x, found[0].w, found[0].score),
            (100.0, 220.0, 0.9)
        );
    }

    #[test]
    fn a_box_over_lines_of_printed_text_is_a_watermark() {
        let line = |x, y, w| Rect { x, y, w, h: 20 };
        // A page: a table of three rows, a greeting, a role under the
        // signature.
        let lines = [
            line(100, 100, 300),
            line(100, 130, 300),
            line(100, 160, 300),
            line(100, 230, 200),
            line(100, 330, 150),
        ];
        let f = |x, y, w, h| Candidate {
            x,
            y,
            w,
            h,
            score: 0.9,
        };
        // A watermark across the table: three rows under it.
        assert!(printed(&f(90.0, 90.0, 400.0, 100.0), &lines));
        // A signature between the greeting and its role crosses both.
        assert!(!printed(&f(90.0, 225.0, 220.0, 120.0), &lines));
        // A line only touched at its edge does not count.
        assert!(!printed(&f(385.0, 90.0, 100.0, 100.0), &lines));
        // The right part of long lines counts: a few words of each.
        assert!(printed(&f(300.0, 90.0, 300.0, 100.0), &lines));
    }

    #[test]
    fn a_box_takes_a_strokes_tail_and_stays_in_the_picture() {
        let f = Candidate {
            x: 100.0,
            y: 100.0,
            w: 200.0,
            h: 100.0,
            score: 0.9,
        };
        assert_eq!(
            place(&f, 1000, 1000),
            Some(Rect {
                x: 90,
                y: 95,
                w: 220,
                h: 110
            })
        );
        let outside = Candidate { x: 2000.0, ..f };
        assert_eq!(place(&outside, 1000, 1000), None);
    }

    #[test]
    fn scores_become_confidences_the_default_threshold_keeps() {
        assert_eq!(confidence(MIN_SCORE), 0.7);
        assert_eq!(confidence(1.0), 1.0);
        assert_eq!(confidence(0.75), 0.85);
    }

    /// The real model, when installed: VEILQ_SIGNATURE_MODEL,
    /// VEILQ_ORT_DYLIB, and a picture VEILQ_SIGNATURE_IMAGE with
    /// VEILQ_SIGNATURE_EXPECT signatures.
    #[test]
    fn the_model_finds_signatures() {
        let (Ok(model), Ok(dylib), Ok(image), Ok(expect)) = (
            std::env::var("VEILQ_SIGNATURE_MODEL"),
            std::env::var("VEILQ_ORT_DYLIB"),
            std::env::var("VEILQ_SIGNATURE_IMAGE"),
            std::env::var("VEILQ_SIGNATURE_EXPECT"),
        ) else {
            return;
        };
        let signatures = Signatures::load(&ModelFiles {
            model: Path::new(&model),
            runtime: Path::new(&dylib),
        })
        .unwrap();
        let img = image::open(image).unwrap().to_rgb8();
        let found = signatures.find(&img, &mut || Ok(Vec::new())).unwrap();
        let strong = found.signatures.iter().filter(|f| f.score >= 0.7).count();
        assert!(
            strong >= expect.parse::<usize>().unwrap(),
            "{strong} signatures: {found:?}"
        );
    }
}
