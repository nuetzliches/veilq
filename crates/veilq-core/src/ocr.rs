//! OCR (ADR-0023): PaddleOCR's text detector and its Latin recogniser in
//! ONNX find the lines of text in an image and every character's place.
//!
//! **Detection** (DB): the image, scaled so that its longer side is at most
//! [`MAX_SIDE`] (a small one enlarged up to [`MAX_UPSCALE`] times) and both
//! sides are multiples of 32, normalised with the
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
/// How far the detector enlarges a small image towards [`MAX_SIDE`]. A
/// screenshot passed on scaled down has lines eight pixels high: read at that
/// size, DB split a line of an address and lost most of the IBAN below it;
/// twice as large, it found both whole.
const MAX_UPSCALE: f32 = 2.0;
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
/// The rows at a picture's top and bottom edge read again for a line the
/// edge cuts (ADR-0059).
const EDGE_STRIP: u32 = 48;
/// The rows of background added beyond the edge for that reading: with
/// them, the detector sees a line cut in half that it misses at the edge.
const EDGE_MARGIN: u32 = 48;
/// How close to the edge a line's box comes to reach it.
const EDGE_TOUCH: u32 = 2;

/// `r` widened to the left and right over what is not `background` in its
/// rows, across gaps as wide as it is high: the detector boxes the rest of
/// a cut line short, and the tops of its last letters stood beside the box.
fn over_ink(img: &RgbImage, r: Rect, background: image::Rgb<u8>) -> Rect {
    let w = img.width();
    let gap = r.h.max(8);
    let rows = r.y..r.bottom().min(img.height());
    let ink = |x: u32| rows.clone().any(|y| *img.get_pixel(x, y) != background);
    let mut right = r.right().min(w);
    let mut x = right;
    while x < w && x < right + gap {
        if ink(x) {
            right = x + 1;
        }
        x += 1;
    }
    let mut left = r.x;
    let mut x = left;
    while x > 0 && x + gap > left {
        x -= 1;
        if ink(x) {
            left = x;
        }
    }
    Rect {
        x: left,
        w: right - left,
        ..r
    }
}

/// Whether `a` holds `b`: they meet across, and at least half of `b`'s rows
/// are `a`'s.
fn holds(a: &Rect, b: &Rect) -> bool {
    let rows = a.bottom().min(b.bottom()).saturating_sub(a.y.max(b.y));
    a.intersects(b) && rows * 2 >= b.h
}

/// The colour most pixels of row `y` of `img` have: the background beside
/// a line the edge cuts.
fn background(img: &RgbImage, y: u32) -> image::Rgb<u8> {
    let mut counts: std::collections::HashMap<[u8; 3], usize> = std::collections::HashMap::new();
    for x in 0..img.width() {
        *counts.entry(img.get_pixel(x, y).0).or_default() += 1;
    }
    let best = counts
        .into_iter()
        .max_by_key(|(c, n)| (*n, *c))
        .map_or([255, 255, 255], |(c, _)| c);
    image::Rgb(best)
}

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
        self.pad_xy(p, p, width, height)
    }

    /// Grown by `px` pixels to the sides and `py` up and down, within
    /// `width` x `height`.
    pub fn pad_xy(&self, px: u32, py: u32, width: u32, height: u32) -> Rect {
        let (x, y) = (self.x.saturating_sub(px), self.y.saturating_sub(py));
        Rect {
            x,
            y,
            w: (self.right() + px).min(width) - x,
            h: (self.bottom() + py).min(height) - y,
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
    /// Cut by the picture's top or bottom edge, found only with a margin
    /// added there (ADR-0059); its text may be empty: none could be read.
    pub cut: bool,
    /// Read as handwriting ([`Htr`]): a second reading of a line read
    /// badly, beside it.
    pub handwriting: bool,
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
    /// Reads a line read badly again, as handwriting (#59).
    htr: Option<Htr>,
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
            htr: None,
        })
    }

    /// This OCR with a handwriting recogniser: a line read badly is read
    /// again by it (#59).
    pub fn with_htr(mut self, htr: Htr) -> Self {
        self.htr = Some(htr);
        self
    }

    /// Whether lines read badly are read again as handwriting.
    pub fn has_htr(&self) -> bool {
        self.htr.is_some()
    }

    /// The lines of text in `img`, top to bottom, left to right; with them
    /// the lines its top and bottom edge cut ([`Line::cut`]), and beside a
    /// line read badly its reading as handwriting ([`Line::handwriting`]).
    pub fn read(&self, img: &RgbImage) -> Result<Vec<Line>, Error> {
        self.read_edges(img, true, true)
    }

    /// [`read`](Self::read), the lines cut looked for at the `top` and the
    /// `bottom` edge only: those that are the picture's, not a band's open
    /// edge (#129).
    pub fn read_edges(&self, img: &RgbImage, top: bool, bottom: bool) -> Result<Vec<Line>, Error> {
        let mut lines = Vec::new();
        for rect in self.detect(img)? {
            if let Some(line) = self.recognise(img, rect)?
                && !line.text.trim().is_empty()
            {
                lines.push(line);
            }
        }
        // A line read already is not read again: one that holds half of a
        // cut line's height. The line above may touch it, and the boxes of
        // one cut line each other.
        let mut cut = Vec::new();
        let read: Vec<Rect> = lines.iter().map(|l| l.rect).collect();
        let edges = [(false, top), (true, bottom)];
        for bottom in edges.into_iter().filter(|e| e.1).map(|e| e.0) {
            for l in self.cut_lines(img, bottom, &read)? {
                if !lines.iter().any(|o| holds(&o.rect, &l.rect)) {
                    cut.push(l);
                }
            }
        }
        // A line read badly is read again as handwriting, from the same box,
        // and both readings are kept: a finding in either is filled (#59).
        // The scores of the two models are not alike; neither replaces the
        // other.
        if let Some(htr) = &self.htr {
            let mut second = Vec::new();
            // At most so many per reading: a picture of thin, wide noise
            // made thousands of calls (security review before 0.7.0). The
            // lines past them stay read badly, as the policy says.
            for l in lines
                .iter()
                .filter(|l| l.weak().next().is_some())
                .take(HTR_LINES)
            {
                if let Some(h) = htr.recognise(img, l.rect)?
                    && !h.text.trim().is_empty()
                    && h.text != l.text
                {
                    second.push(h);
                }
            }
            lines.extend(second);
        }
        lines.extend(cut);
        lines.sort_by_key(|l| (l.rect.y + l.rect.h / 2, l.rect.x));
        Ok(lines)
    }

    /// The lines the top (or `bottom`) edge of `img` cuts. The detector does
    /// not see a line cut in half there (ADR-0059): the rows at the edge are
    /// read again with [`EDGE_MARGIN`] rows of the edge's background beyond
    /// them and the lines `read` already painted over with it (the rest of
    /// a cut line otherwise joins the box of the line above), and a line
    /// that reaches the edge is kept, read or not.
    fn cut_lines(&self, img: &RgbImage, bottom: bool, read: &[Rect]) -> Result<Vec<Line>, Error> {
        let (w, h) = img.dimensions();
        let strip = h.min(EDGE_STRIP);
        let edge_row = if bottom { h - 1 } else { 0 };
        let background = background(img, edge_row);
        let mut padded = RgbImage::from_pixel(w, strip + EDGE_MARGIN, background);
        let (from, to) = if bottom {
            (h - strip, 0)
        } else {
            (0, EDGE_MARGIN)
        };
        imageops::replace(
            &mut padded,
            &imageops::crop_imm(img, 0, from, w, strip).to_image(),
            0,
            i64::from(to),
        );
        for r in read {
            // In the strip's rows, without the edge's own: a line read there
            // is not cut.
            let (top, end) = (r.y.max(from), r.bottom().min(from + strip));
            if top >= end || (if bottom { end >= h } else { top == 0 }) {
                continue;
            }
            for y in top..end {
                for x in r.x..r.right().min(w) {
                    padded.put_pixel(x, y - from + to, background);
                }
            }
        }
        let mut out = Vec::new();
        for rect in self.detect(&padded)? {
            let reaches = if bottom {
                rect.bottom() + EDGE_TOUCH >= strip
            } else {
                rect.y <= EDGE_MARGIN + EDGE_TOUCH
            };
            // Wholly in the margin, or not at the edge.
            let inside = if bottom {
                rect.y < strip
            } else {
                rect.bottom() > EDGE_MARGIN
            };
            if !reaches || !inside {
                continue;
            }
            let rect = over_ink(&padded, rect, background);
            let line = self.recognise(&padded, rect)?;
            // Back to the picture: rows of the margin are none of its.
            let place = |r: Rect| {
                let (top, end) = if bottom {
                    (from + r.y, (from + r.bottom()).min(h))
                } else {
                    (
                        r.y.saturating_sub(EDGE_MARGIN),
                        r.bottom().saturating_sub(EDGE_MARGIN).min(h),
                    )
                };
                Rect {
                    y: top,
                    h: end.saturating_sub(top).max(1),
                    ..r
                }
            };
            out.push(match line {
                Some(l) if !l.text.trim().is_empty() => Line {
                    rect: place(l.rect),
                    chars: l
                        .chars
                        .into_iter()
                        .map(|(s, e, r)| (s, e, place(r)))
                        .collect(),
                    cut: true,
                    handwriting: false,
                    ..l
                },
                _ => Line {
                    text: String::new(),
                    rect: place(rect),
                    chars: Vec::new(),
                    score: 0.0,
                    scores: Vec::new(),
                    cut: true,
                    handwriting: false,
                },
            });
        }
        Ok(out)
    }

    fn detect(&self, img: &RgbImage) -> Result<Vec<Rect>, Error> {
        let (w, h) = img.dimensions();
        let (sw, sh) = detect_size(w, h);
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
        Ok(decode(probs, steps, classes, &self.alphabet, rect))
    }
}

/// The line read from `rect`: greedy CTC over `probs`, `steps` rows of
/// `classes` probabilities, class 0 the blank and class `i` the
/// `alphabet`'s `i`th. Per step the best class; a run of one class is one
/// character, a blank ends a run. `None` when nothing was read.
fn decode(
    probs: &[f32],
    steps: usize,
    classes: usize,
    alphabet: &[String],
    rect: Rect,
) -> Option<Line> {
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
        return None;
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
        text.push_str(&alphabet[*class]);
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
    Some(Line {
        text,
        rect,
        chars,
        score,
        scores: runs.iter().map(|r| r.3).collect(),
        cut: false,
        handwriting: false,
    })
}

/// A recogniser of handwritten lines (#59, M5): `naeyn/de-htr-web-v2`, a
/// CTC model for modern German handwriting (ResNet18 and BiLSTM, int8,
/// Apache-2.0). It reads a line PaddleOCR read badly a second time, from
/// the same box: grey, [`HTR_HEIGHT`] high, white padding, at most
/// [`HTR_WIDTH`] wide per piece; each step covers [`HTR_STRIDE`] columns.
pub struct Htr {
    session: Mutex<Session>,
    /// Index 0 is the blank.
    alphabet: Vec<String>,
}

/// The files of an installed handwriting recogniser.
pub struct HtrFiles<'a> {
    pub model: &'a Path,
    /// Its `config.json`, which holds the alphabet.
    pub config: &'a Path,
    pub runtime: &'a Path,
}

const HTR_HEIGHT: u32 = 64;
/// The lines read badly that one reading of a picture reads again as
/// handwriting: a hand-filled form has a few dozen.
const HTR_LINES: usize = 64;
const HTR_WIDTH: u32 = 1024;
const HTR_STRIDE: u32 = 4;

impl Htr {
    pub fn load(files: &HtrFiles) -> Result<Self, Error> {
        let env = crate::runtime::env(files.runtime)?;
        let session = Session::builder(env)
            .map_err(error)?
            .commit_from_file(files.model)
            .map_err(|e| error(format!("{}: {e}", files.model.display())))?;
        let config = std::fs::read_to_string(files.config)
            .map_err(|e| error(format!("{}: {e}", files.config.display())))?;
        let config: serde_json::Value = serde_json::from_str(&config)
            .map_err(|e| error(format!("{}: {e}", files.config.display())))?;
        let shape = |k: &str| config.get(k).and_then(serde_json::Value::as_u64);
        if (
            shape("image_height"),
            shape("output_stride"),
            shape("blank_index"),
        ) != (
            Some(u64::from(HTR_HEIGHT)),
            Some(u64::from(HTR_STRIDE)),
            Some(0),
        ) {
            return Err(error(format!(
                "{}: not the handwriting recogniser veilq reads",
                files.config.display()
            )));
        }
        let alphabet = config
            .get("alphabet")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| error(format!("{}: no alphabet", files.config.display())))?;
        let mut letters = vec![String::new()];
        letters.extend(alphabet.chars().map(String::from));
        Ok(Self {
            session: Mutex::new(session),
            alphabet: letters,
        })
    }

    /// The line in `rect` of `img` read as handwriting; `None` when nothing
    /// was read. A line wider than [`HTR_WIDTH`] at this height is read in
    /// pieces, put together again.
    pub fn recognise(&self, img: &RgbImage, rect: Rect) -> Result<Option<Line>, Error> {
        let crop = imageops::crop_imm(img, rect.x, rect.y, rect.w, rect.h).to_image();
        let width = ((HTR_HEIGHT as f32 * rect.w as f32 / rect.h.max(1) as f32).ceil() as u32)
            .clamp(HTR_STRIDE, 16 * HTR_WIDTH);
        let grey = imageops::grayscale(&imageops::resize(
            &crop,
            width,
            HTR_HEIGHT,
            imageops::FilterType::Triangle,
        ));
        let mut out: Option<Line> = None;
        let mut x = 0;
        // Whether the piece before ended in a blank column.
        let mut gap = false;
        while x < width {
            let (piece, blank) = cut(&grey, x, width);
            let padded = piece.div_ceil(HTR_STRIDE) * HTR_STRIDE;
            let plane = (padded * HTR_HEIGHT) as usize;
            let mut data = vec![1f32; plane];
            for y in 0..HTR_HEIGHT {
                for i in 0..piece {
                    data[(y * padded + i) as usize] =
                        f32::from(grey.get_pixel(x + i, y)[0]) / 255.0;
                }
            }
            let input =
                Tensor::from_array(([1usize, 1, HTR_HEIGHT as usize, padded as usize], data))
                    .map_err(error)?;
            let mut session = self
                .session
                .lock()
                .map_err(|_| error("a poisoned session"))?;
            let outputs = session.run(ort::inputs![input]).map_err(error)?;
            let (shape, logits) = outputs[0].try_extract_tensor::<f32>().map_err(error)?;
            // [1, steps, classes], or [steps, 1, classes].
            let (steps, classes) = match shape.iter().map(|d| *d as usize).collect::<Vec<_>>()[..] {
                [1, s, c] | [s, 1, c] => (s, c),
                _ => return Err(error(format!("the handwriting recogniser gives {shape:?}"))),
            };
            if steps == 0 || classes != self.alphabet.len() {
                return Err(error(format!(
                    "the handwriting recogniser gives {classes} classes, its alphabet {}",
                    self.alphabet.len()
                )));
            }
            let probs = softmax(logits, classes);
            // The piece's box in the picture: its columns of the line.
            let scale = rect.w as f32 / width as f32;
            let x0 = rect.x + (x as f32 * scale) as u32;
            let x1 = (rect.x + ((x + padded) as f32 * scale).ceil() as u32).min(rect.right());
            let part = Rect {
                x: x0,
                w: (x1 - x0).max(1),
                ..rect
            };
            if let Some(line) = decode(&probs, steps, classes, &self.alphabet, part) {
                out = Some(match out {
                    None => line,
                    Some(mut all) => {
                        // Cut where the line was blank: a gap between words.
                        if gap && !all.text.ends_with(' ') && !line.text.starts_with(' ') {
                            all.text.push(' ');
                        }
                        let base = all.text.len();
                        all.text.push_str(&line.text);
                        all.chars.extend(
                            line.chars
                                .into_iter()
                                .map(|(s, e, r)| (s + base, e + base, r)),
                        );
                        all.scores.extend(line.scores);
                        all.score = all.scores.iter().sum::<f32>() / all.scores.len() as f32;
                        all
                    }
                });
            }
            x += piece;
            gap = blank;
        }
        Ok(out.map(|l| Line {
            rect,
            handwriting: true,
            ..l
        }))
    }
}

/// Lighter than this, a column of the grey line holds no ink.
const BLANK: u8 = 200;

/// How many columns of `grey` from `x` on, up to `width`, are read as one
/// piece: all that is left when it fits [`HTR_WIDTH`], else up to the last
/// blank column in its last quarter, so that no character is cut in two
/// (#137 review); a line without one there is cut at the limit. Whether the
/// cut lies in a blank column.
fn cut(grey: &image::GrayImage, x: u32, width: u32) -> (u32, bool) {
    if width - x <= HTR_WIDTH {
        return (width - x, false);
    }
    let blank = |c: u32| (0..grey.height()).all(|y| grey.get_pixel(x + c, y)[0] >= BLANK);
    match (HTR_WIDTH * 3 / 4..HTR_WIDTH).rev().find(|&c| blank(c)) {
        Some(c) => (c + 1, true),
        None => (HTR_WIDTH, false),
    }
}

/// Each row of `classes` logits as probabilities.
fn softmax(logits: &[f32], classes: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(logits.len());
    for row in logits.chunks(classes) {
        let max = row.iter().copied().fold(f32::MIN, f32::max);
        let exp: Vec<f32> = row.iter().map(|v| (v - max).exp()).collect();
        let sum: f32 = exp.iter().sum();
        out.extend(exp.into_iter().map(|e| e / sum));
    }
    out
}

/// The size the detector reads an image of `w` x `h` at: its longer side
/// towards [`MAX_SIDE`], enlarged at most [`MAX_UPSCALE`] times, both sides
/// multiples of 32.
fn detect_size(w: u32, h: u32) -> (u32, u32) {
    let scale = (MAX_SIDE as f32 / w.max(h) as f32).min(MAX_UPSCALE);
    let round32 = |v: f32| ((v / 32.0).round() as u32).max(1) * 32;
    (round32(w as f32 * scale), round32(h as f32 * scale))
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
    fn a_small_image_is_read_larger_a_large_one_smaller() {
        // A screenshot passed on at 800 pixels: twice its size.
        assert_eq!(detect_size(800, 545), (1600, 1088));
        // Not more than twice, however small.
        assert_eq!(detect_size(400, 200), (800, 416));
        // Between: up to the longer side the detector reads.
        assert_eq!(detect_size(1200, 600), (1600, 800));
        // Larger: down to it, as before.
        assert_eq!(detect_size(3200, 1600), (1600, 800));
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

    #[test]
    fn a_long_line_is_cut_in_a_blank_column() {
        // Ink everywhere but one blank column in the piece's last quarter.
        let w = HTR_WIDTH + 300;
        let mut grey = image::GrayImage::from_pixel(w, HTR_HEIGHT, image::Luma([20]));
        for y in 0..HTR_HEIGHT {
            grey.put_pixel(900, y, image::Luma([250]));
        }
        assert_eq!(cut(&grey, 0, w), (901, true));
        // None there: at the limit; what fits, whole.
        assert_eq!(cut(&grey, 901, w), (w - 901, false));
        let full = image::GrayImage::from_pixel(w, HTR_HEIGHT, image::Luma([20]));
        assert_eq!(cut(&full, 0, w), (HTR_WIDTH, false));
    }

    #[test]
    fn logits_become_probabilities_per_step() {
        let p = softmax(&[0.0, 0.0, 2.0, 1.0, 1.0, 1.0], 3);
        assert!((p[0] - p[1]).abs() < 1e-6 && p[2] > 0.7);
        assert!((p[3..].iter().sum::<f32>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_ctc_path_gives_characters_with_their_columns() {
        // Steps of three classes (blank, `a`, `b`) over a box 40 wide.
        let alphabet: Vec<String> = ["", "a", "b"].map(String::from).to_vec();
        let steps = [
            [0.1, 0.8, 0.1],
            [0.1, 0.8, 0.1],
            [0.9, 0.05, 0.05],
            [0.1, 0.1, 0.8],
        ];
        let probs: Vec<f32> = steps.iter().flatten().copied().collect();
        let rect = Rect {
            x: 100,
            y: 10,
            w: 40,
            h: 20,
        };
        let line = decode(&probs, 4, 3, &alphabet, rect).unwrap();
        assert_eq!(line.text, "ab");
        assert_eq!(line.chars.len(), 2);
        assert!(line.chars[0].2.x >= 100 && line.chars[1].2.right() <= 140);
        assert!(!line.handwriting);
    }

    /// The real recogniser, when installed: VEILQ_HTR_DIR (model.onnx,
    /// config.json), VEILQ_ORT_DYLIB, and a picture of one handwritten line
    /// VEILQ_HTR_IMAGE that reads VEILQ_HTR_EXPECT.
    #[test]
    fn the_handwriting_recogniser_reads_a_line() {
        let (Ok(dir), Ok(dylib), Ok(image), Ok(expect)) = (
            std::env::var("VEILQ_HTR_DIR"),
            std::env::var("VEILQ_ORT_DYLIB"),
            std::env::var("VEILQ_HTR_IMAGE"),
            std::env::var("VEILQ_HTR_EXPECT"),
        ) else {
            return;
        };
        let dir = Path::new(&dir);
        let htr = Htr::load(&HtrFiles {
            model: &dir.join("model.onnx"),
            config: &dir.join("config.json"),
            runtime: Path::new(&dylib),
        })
        .unwrap();
        let img = image::open(image).unwrap().to_rgb8();
        let (w, h) = img.dimensions();
        let line = htr
            .recognise(&img, Rect { x: 0, y: 0, w, h })
            .unwrap()
            .unwrap();
        assert!(line.handwriting);
        assert!(line.text.contains(&expect), "{:?}", line.text);
    }

    #[test]
    fn a_line_holds_a_cut_line_by_half_its_height() {
        let r = |y, h| Rect { x: 0, y, w: 100, h };
        assert!(holds(&r(1022, 18), &r(1030, 10)));
        // The line above touches the cut line by two rows.
        assert!(!holds(&r(1022, 18), &r(1038, 9)));
        assert!(!holds(
            &Rect {
                x: 200,
                ..r(1022, 18)
            },
            &r(1030, 10)
        ));
    }

    #[test]
    fn a_cut_lines_box_grows_over_its_ink() {
        let white = image::Rgb([255, 255, 255]);
        let mut img = RgbImage::from_pixel(100, 10, white);
        // Letters at 10..20, 24..30 and 60..62; the box holds 12..18.
        for x in (10..20).chain(24..30).chain(60..62) {
            img.put_pixel(x, 5, image::Rgb([0, 0, 0]));
        }
        let r = over_ink(
            &img,
            Rect {
                x: 12,
                y: 2,
                w: 6,
                h: 8,
            },
            white,
        );
        assert_eq!((r.x, r.right()), (10, 30), "not across a wide gap");
    }
}
