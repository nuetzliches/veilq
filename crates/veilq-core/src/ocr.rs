//! OCR (ADR-0023): PaddleOCR's text detector and its Latin recogniser in
//! ONNX find the lines of text in an image and every character's place.
//!
//! **Detection** (DB): the image, scaled so that its longer side is at most
//! [`MAX_SIDE`] and both sides are multiples of 32, normalised with the
//! ImageNet mean and deviation, gives a map of text probability; regions above
//! [`PIXEL`] that score at least [`BOX`] become boxes, widened as DB widens
//! them (`unclip`). **Recognition** (CTC): each box, scaled to a height of 48,
//! gives per time step a distribution over blank, the dictionary and a space;
//! the greedy path collapses repeats and drops blanks. The time steps a
//! character holds give its horizontal extent, so a finding inside a line is
//! blacked out alone, not the whole line. Its probability on that path is its
//! score; a character below [`WEAK`] was read badly (#52).

use std::path::Path;
use std::sync::Mutex;

use image::{RgbImage, imageops};
use ort::session::Session;
use ort::value::Tensor;

use crate::Error;

/// The longer side of the image the detector reads.
pub const MAX_SIDE: u32 = 1600;
/// Text probability a pixel needs.
const PIXEL: f32 = 0.3;
/// Mean probability a box needs.
const BOX: f32 = 0.5;
/// How far DB widens a box: area x ratio / perimeter.
const UNCLIP: f32 = 1.6;
const REC_HEIGHT: u32 = 48;
/// The score below which a letter or digit counts as read badly. Measured
/// (#52): printed text kept every letter and digit at 0.73 and above (an
/// `ä` in a screen recording's 13-pixel sidebar the lowest); handwriting-style
/// fonts and small text in a scaled screenshot gave lines with letters and
/// digits from 0.16 to 0.63, misread ones among them. A line's mean does not
/// tell: misread lines averaged 0.95. Punctuation is left out, as printed
/// separators (`·`) scored down to 0.53 and hold no personal data.
pub const WEAK: f32 = 0.7;

fn error(message: impl std::fmt::Display) -> Error {
    Error::Format(format!("OCR: {message}"))
}

/// A rectangle in pixels of the original image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn right(&self) -> u32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        Rect {
            x,
            y,
            w: self.right().max(o.right()) - x,
            h: self.bottom().max(o.bottom()) - y,
        }
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }

    /// Grown by `p` pixels on every side, within `width` x `height`.
    pub fn pad(&self, p: u32, width: u32, height: u32) -> Rect {
        let (x, y) = (self.x.saturating_sub(p), self.y.saturating_sub(p));
        Rect {
            x,
            y,
            w: (self.right() + p).min(width) - x,
            h: (self.bottom() + p).min(height) - y,
        }
    }
}

/// A recognised line: its text, its box, and per character its byte range
/// in the text and its box.
#[derive(Debug, Clone)]
pub struct Line {
    pub text: String,
    pub rect: Rect,
    pub chars: Vec<(usize, usize, Rect)>,
    pub score: f32,
    /// Per character, as `chars`, its score.
    pub scores: Vec<f32>,
}

impl Line {
    /// The boxes of the letters and digits read badly (below [`WEAK`]); none
    /// in a line of one character, which is mostly an icon or a cursor.
    pub fn weak(&self) -> impl Iterator<Item = &Rect> {
        let many = self.chars.len() > 1;
        self.chars
            .iter()
            .zip(&self.scores)
            .filter(move |((start, end, _), s)| {
                many && **s < WEAK && self.text[*start..*end].chars().all(char::is_alphanumeric)
            })
            .map(|((_, _, r), _)| r)
    }
}

pub struct Ocr {
    det: Mutex<Session>,
    rec: Mutex<Session>,
    /// Index 0 is the blank; the last, a space.
    alphabet: Vec<String>,
}

/// The files of an installed OCR.
pub struct ModelFiles<'a> {
    pub det: &'a Path,
    pub rec: &'a Path,
    pub dict: &'a Path,
    pub runtime: &'a Path,
}

impl Ocr {
    pub fn load(files: &ModelFiles) -> Result<Self, Error> {
        let env = crate::runtime::env(files.runtime)?;
        let session = |p: &Path| {
            Session::builder(env)
                .map_err(error)?
                .commit_from_file(p)
                .map_err(|e| error(format!("{}: {e}", p.display())))
        };
        let dict = std::fs::read_to_string(files.dict)
            .map_err(|e| error(format!("{}: {e}", files.dict.display())))?;
        let mut alphabet = vec![String::new()];
        alphabet.extend(dict.lines().map(str::to_owned));
        alphabet.push(" ".into());
        Ok(Self {
            det: Mutex::new(session(files.det)?),
            rec: Mutex::new(session(files.rec)?),
            alphabet,
        })
    }

    /// The lines of text in `img`, top to bottom, left to right.
    pub fn read(&self, img: &RgbImage) -> Result<Vec<Line>, Error> {
        let mut lines = Vec::new();
        for rect in self.detect(img)? {
            if let Some(line) = self.recognise(img, rect)?
                && !line.text.trim().is_empty()
            {
                lines.push(line);
            }
        }
        lines.sort_by_key(|l| (l.rect.y + l.rect.h / 2, l.rect.x));
        Ok(lines)
    }

    fn detect(&self, img: &RgbImage) -> Result<Vec<Rect>, Error> {
        let (w, h) = img.dimensions();
        let scale = (MAX_SIDE as f32 / w.max(h) as f32).min(1.0);
        let round32 = |v: f32| ((v / 32.0).round() as u32).max(1) * 32;
        let (sw, sh) = (round32(w as f32 * scale), round32(h as f32 * scale));
        let small = imageops::resize(img, sw, sh, imageops::FilterType::Triangle);
        const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
        const STD: [f32; 3] = [0.229, 0.224, 0.225];
        let plane = (sw * sh) as usize;
        let mut data = vec![0f32; 3 * plane];
        for (i, p) in small.pixels().enumerate() {
            for c in 0..3 {
                data[c * plane + i] = (f32::from(p[c]) / 255.0 - MEAN[c]) / STD[c];
            }
        }
        let mut det = self.det.lock().map_err(|_| error("a poisoned session"))?;
        let input =
            Tensor::from_array(([1usize, 3, sh as usize, sw as usize], data)).map_err(error)?;
        let outputs = det.run(ort::inputs![input]).map_err(error)?;
        let (shape, prob) = outputs[0].try_extract_tensor::<f32>().map_err(error)?;
        let (mh, mw) = (shape[2] as usize, shape[3] as usize);
        let (fx, fy) = (w as f32 / mw as f32, h as f32 / mh as f32);
        Ok(components(prob, mw, mh)
            .into_iter()
            .filter(|c| c.score >= BOX && c.w >= 3 && c.h >= 3)
            .map(|c| {
                // DB's unclip on an axis-aligned box: widen by area x r / perimeter.
                let (bw, bh) = (c.w as f32, c.h as f32);
                let d = bw * bh * UNCLIP / (2.0 * (bw + bh));
                let x0 = ((c.x as f32 - d) * fx).max(0.0);
                let y0 = ((c.y as f32 - d) * fy).max(0.0);
                let x1 = (((c.x + c.w) as f32 + d) * fx).min(w as f32);
                let y1 = (((c.y + c.h) as f32 + d) * fy).min(h as f32);
                Rect {
                    x: x0 as u32,
                    y: y0 as u32,
                    w: (x1 - x0).max(1.0) as u32,
                    h: (y1 - y0).max(1.0) as u32,
                }
            })
            .collect())
    }

    fn recognise(&self, img: &RgbImage, rect: Rect) -> Result<Option<Line>, Error> {
        let crop = imageops::crop_imm(img, rect.x, rect.y, rect.w, rect.h).to_image();
        let width =
            ((REC_HEIGHT as f32 * rect.w as f32 / rect.h as f32).ceil() as u32).clamp(8, 4000);
        let resized = imageops::resize(&crop, width, REC_HEIGHT, imageops::FilterType::Triangle);
        let plane = (width * REC_HEIGHT) as usize;
        let mut data = vec![0f32; 3 * plane];
        for (i, p) in resized.pixels().enumerate() {
            for c in 0..3 {
                data[c * plane + i] = (f32::from(p[c]) / 255.0 - 0.5) / 0.5;
            }
        }
        let mut rec = self.rec.lock().map_err(|_| error("a poisoned session"))?;
        let input = Tensor::from_array(([1usize, 3, REC_HEIGHT as usize, width as usize], data))
            .map_err(error)?;
        let outputs = rec.run(ort::inputs![input]).map_err(error)?;
        let (shape, probs) = outputs[0].try_extract_tensor::<f32>().map_err(error)?;
        let (steps, classes) = (shape[1] as usize, shape[2] as usize);
        if steps == 0 || classes != self.alphabet.len() {
            return Err(error(format!(
                "the recogniser gives {classes} classes, the dictionary {}",
                self.alphabet.len()
            )));
        }
        // Greedy CTC: per step the best class; a run of one class is one
        // character, a blank ends a run.
        let mut runs: Vec<(usize, usize, usize, f32)> = Vec::new(); // class, first, last, score
        let mut prev = 0;
        for t in 0..steps {
            let row = &probs[t * classes..(t + 1) * classes];
            let (best, p) =
                row.iter().enumerate().fold(
                    (0, f32::MIN),
                    |a, (i, v)| if *v > a.1 { (i, *v) } else { a },
                );
            if best != 0 {
                if best == prev
                    && let Some(last) = runs.last_mut()
                {
                    last.2 = t;
                    last.3 = last.3.max(p);
                } else {
                    runs.push((best, t, t, p));
                }
            }
            prev = best;
        }
        if runs.is_empty() {
            return Ok(None);
        }
        let step = rect.w as f32 / steps as f32;
        let mut text = String::new();
        let mut chars = Vec::new();
        for (k, (class, first, _, _)) in runs.iter().enumerate() {
            // A character reaches from its first step to the next one's.
            let end_step = runs.get(k + 1).map_or(steps, |n| n.1);
            // A step more on each side: CTC places a character's first step
            // after its left edge, and a box must not leave a stroke.
            let x0 = rect.x + ((*first as f32 - 1.0).max(0.0) * step) as u32;
            let x1 = (rect.x + ((end_step as f32 + 1.0) * step).ceil() as u32).min(rect.right());
            let start = text.len();
            text.push_str(&self.alphabet[*class]);
            chars.push((
                start,
                text.len(),
                Rect {
                    x: x0,
                    y: rect.y,
                    w: (x1 - x0).max(1),
                    h: rect.h,
                },
            ));
        }
        let score = runs.iter().map(|r| r.3).sum::<f32>() / runs.len() as f32;
        Ok(Some(Line {
            text,
            rect,
            chars,
            score,
            scores: runs.iter().map(|r| r.3).collect(),
        }))
    }
}

/// A connected region of the probability map above [`PIXEL`].
#[derive(Debug, PartialEq)]
struct Component {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    score: f32,
}

/// The 4-connected regions of `prob` (`w` x `h`) above [`PIXEL`], with their
/// mean probability.
fn components(prob: &[f32], w: usize, h: usize) -> Vec<Component> {
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if seen[start] || prob[start] < PIXEL {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        let (mut sum, mut n) = (0f32, 0usize);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            sum += prob[i];
            n += 1;
            let mut push = |j: usize| {
                if !seen[j] && prob[j] >= PIXEL {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < w {
                push(i + 1);
            }
            if y > 0 {
                push(i - w);
            }
            if y + 1 < h {
                push(i + w);
            }
        }
        out.push(Component {
            x: x0,
            y: y0,
            w: x1 - x0 + 1,
            h: y1 - y0 + 1,
            score: sum / n as f32,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_of_the_probability_map() {
        // Two blobs in a 6 x 3 map.
        #[rustfmt::skip]
        let prob = [
            0.9, 0.9, 0.0, 0.0, 0.0, 0.0,
            0.9, 0.9, 0.0, 0.0, 0.8, 0.8,
            0.0, 0.0, 0.0, 0.0, 0.8, 0.8,
        ];
        let c = components(&prob, 6, 3);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].x, c[0].y, c[0].w, c[0].h), (0, 0, 2, 2));
        assert_eq!((c[1].x, c[1].y, c[1].w, c[1].h), (4, 1, 2, 2));
        assert!((c[1].score - 0.8).abs() < 1e-6);
    }

    #[test]
    fn rects() {
        let a = Rect {
            x: 10,
            y: 10,
            w: 10,
            h: 10,
        };
        let b = Rect {
            x: 15,
            y: 5,
            w: 20,
            h: 10,
        };
        assert!(a.intersects(&b));
        assert_eq!(
            a.union(&b),
            Rect {
                x: 10,
                y: 5,
                w: 25,
                h: 15
            }
        );
        assert_eq!(
            a.pad(15, 100, 25),
            Rect {
                x: 0,
                y: 0,
                w: 35,
                h: 25
            }
        );
    }

    /// The real models, when installed: VEILQ_OCR_DIR (det.onnx, rec.onnx,
    /// dict.txt), VEILQ_ORT_DYLIB, and an image VEILQ_OCR_IMAGE that shows
    /// the text VEILQ_OCR_EXPECT.
    #[test]
    fn the_models_read_an_image() {
        let (Ok(dir), Ok(dylib), Ok(image), Ok(expect)) = (
            std::env::var("VEILQ_OCR_DIR"),
            std::env::var("VEILQ_ORT_DYLIB"),
            std::env::var("VEILQ_OCR_IMAGE"),
            std::env::var("VEILQ_OCR_EXPECT"),
        ) else {
            return;
        };
        let dir = Path::new(&dir);
        let ocr = Ocr::load(&ModelFiles {
            det: &dir.join("det.onnx"),
            rec: &dir.join("rec.onnx"),
            dict: &dir.join("dict.txt"),
            runtime: Path::new(&dylib),
        })
        .unwrap();
        let img = image::open(image).unwrap().to_rgb8();
        let text: Vec<String> = ocr
            .read(&img)
            .unwrap()
            .into_iter()
            .map(|l| l.text)
            .collect();
        let all = text.join("\n");
        for e in expect.split('|') {
            assert!(all.contains(e), "{e:?} not in {all:?}");
        }
    }
}
