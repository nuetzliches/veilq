//! Faces in pictures (ADR-0038): YuNet finds them, veilq fills them black.
//! It finds faces only; it never tells whose a face is (no features, no
//! comparison), and nothing leaves the machine.
//!
//! YuNet reads 640 x 640 pixels (BGR, 0 to 255), padded at the right and
//! bottom. Per anchor of the strides 8, 16 and 32 it gives a class and an
//! object score, whose geometric mean is the face's score, and a box
//! relative to the anchor. It finds faces from about 10 to 300 pixels in
//! what it reads, so a picture is read as a pyramid: whole, scaled to fit,
//! then at twice that scale and so on up to its own size, each scale in
//! tiles of 640 that overlap by [`OVERLAP`]. A face of 20 pixels in a
//! screenshot is read at full size, a portrait that fills a photo scaled
//! down.

use std::path::Path;
use std::sync::Mutex;

use image::{RgbImage, imageops};
use ort::session::Session;
use ort::value::Tensor;

use crate::Error;
use crate::ocr::Rect;

/// The side YuNet reads.
const SIDE: u32 = 640;
/// Pixels two tiles share: a face up to this size is whole in one of them.
const OVERLAP: u32 = 160;
/// The detector's own threshold: low, since a hand filled black costs less
/// than a face left (ADR-0038).
pub const MIN_SCORE: f32 = 0.5;
/// Two boxes of one face, from neighbouring anchors or scales: they overlap
/// by this much (of their union), or the smaller lies this much inside.
const SAME_IOU: f32 = 0.3;
const SAME_INSIDE: f32 = 0.7;
/// A box is grown by this share of its size on every side: YuNet's box
/// ends at the brow and chin, the hairline and ears are part of a face.
const MARGIN: f32 = 0.15;
const STRIDES: [usize; 3] = [8, 16, 32];

fn error(message: impl std::fmt::Display) -> Error {
    Error::Format(format!("face detection: {message}"))
}

/// A face: its box in the picture, grown by [`MARGIN`], and its score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub rect: Rect,
    pub score: f32,
}

/// The files of an installed face detector.
pub struct ModelFiles<'a> {
    pub model: &'a Path,
    pub runtime: &'a Path,
}

pub struct Faces {
    session: Mutex<Session>,
}

/// A box in pixels of what was read, before it is placed in the picture.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Found {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    score: f32,
}

impl Faces {
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

    /// The faces in `img`, the best first.
    pub fn find(&self, img: &RgbImage) -> Result<Vec<Face>, Error> {
        let (w, h) = img.dimensions();
        let mut found = Vec::new();
        for scale in scales(w, h) {
            let (sw, sh) = (
                ((w as f32 * scale).round() as u32).max(1),
                ((h as f32 * scale).round() as u32).max(1),
            );
            let scaled;
            let src = if scale < 1.0 {
                scaled = imageops::resize(img, sw, sh, imageops::FilterType::Triangle);
                &scaled
            } else {
                img
            };
            for ty in starts(sh) {
                for tx in starts(sw) {
                    let tile =
                        imageops::crop_imm(src, tx, ty, SIDE.min(sw - tx), SIDE.min(sh - ty))
                            .to_image();
                    for f in self.read(&tile)? {
                        found.push(Found {
                            x: (f.x + tx as f32) / scale,
                            y: (f.y + ty as f32) / scale,
                            w: f.w / scale,
                            h: f.h / scale,
                            score: f.score,
                        });
                    }
                }
            }
        }
        Ok(distinct(found)
            .into_iter()
            .filter_map(|f| {
                place(&f, w, h).map(|rect| Face {
                    rect,
                    score: f.score,
                })
            })
            .collect())
    }

    /// The faces in one tile of at most [`SIDE`] x [`SIDE`].
    fn read(&self, tile: &RgbImage) -> Result<Vec<Found>, Error> {
        let side = SIDE as usize;
        let plane = side * side;
        let mut data = vec![0f32; 3 * plane];
        for (x, y, p) in tile.enumerate_pixels() {
            let i = y as usize * side + x as usize;
            // BGR, as OpenCV reads it.
            for c in 0..3 {
                data[c * plane + i] = f32::from(p[2 - c]);
            }
        }
        let input = Tensor::from_array(([1usize, 3, side, side], data)).map_err(error)?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| error("a poisoned session"))?;
        let outputs = session.run(ort::inputs![input]).map_err(error)?;
        let mut found = Vec::new();
        for stride in STRIDES {
            let tensor = |name: String| {
                outputs
                    .get(&name)
                    .ok_or_else(|| error(format!("the model has no output `{name}`")))?
                    .try_extract_tensor::<f32>()
                    .map(|(_, v)| v.to_vec())
                    .map_err(error)
            };
            found.extend(decode(
                stride,
                &tensor(format!("cls_{stride}"))?,
                &tensor(format!("obj_{stride}"))?,
                &tensor(format!("bbox_{stride}"))?,
            ));
        }
        Ok(found)
    }
}

/// The scales a `w` x `h` picture is read at: fitting [`SIDE`], then twice
/// that, up to 1 (a picture is never enlarged).
fn scales(w: u32, h: u32) -> Vec<f32> {
    let longer = w.max(h) as f32;
    let mut out = Vec::new();
    let mut fit = SIDE as f32;
    loop {
        let s = (fit / longer).min(1.0);
        out.push(s);
        if s >= 1.0 {
            return out;
        }
        fit *= 2.0;
    }
}

/// Where tiles of [`SIDE`] start along `len`, overlapping by [`OVERLAP`];
/// the last one ends at `len`.
fn starts(len: u32) -> Vec<u32> {
    if len <= SIDE {
        return vec![0];
    }
    let step = SIDE - OVERLAP;
    let mut out: Vec<u32> = (0..)
        .map(|i| i * step)
        .take_while(|s| s + SIDE < len)
        .collect();
    out.push(len - SIDE);
    out
}

/// The faces of one stride's outputs, in pixels of the tile.
fn decode(stride: usize, cls: &[f32], obj: &[f32], bbox: &[f32]) -> Vec<Found> {
    let cols = SIDE as usize / stride;
    let s = stride as f32;
    let mut out = Vec::new();
    for (i, (c, o)) in cls.iter().zip(obj).enumerate() {
        let score = (c.clamp(0.0, 1.0) * o.clamp(0.0, 1.0)).sqrt();
        if score < MIN_SCORE || 4 * i + 3 >= bbox.len() {
            continue;
        }
        let (row, col) = ((i / cols) as f32, (i % cols) as f32);
        let b = &bbox[4 * i..4 * i + 4];
        let (cx, cy) = ((col + b[0]) * s, (row + b[1]) * s);
        let (w, h) = (b[2].exp() * s, b[3].exp() * s);
        out.push(Found {
            x: cx - w / 2.0,
            y: cy - h / 2.0,
            w,
            h,
            score,
        });
    }
    out
}

/// One box per face: the best first, a box that is the same face as a
/// better one dropped.
fn distinct(mut found: Vec<Found>) -> Vec<Found> {
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut out: Vec<Found> = Vec::new();
    for f in found {
        if !out.iter().any(|k| same(k, &f)) {
            out.push(f);
        }
    }
    out
}

fn same(a: &Found, b: &Found) -> bool {
    let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    if w <= 0.0 || h <= 0.0 {
        return false;
    }
    let inter = w * h;
    let (aa, ab) = (a.w * a.h, b.w * b.h);
    inter / (aa + ab - inter) >= SAME_IOU || inter / aa.min(ab) >= SAME_INSIDE
}

/// `f` grown by [`MARGIN`] and cut to the picture; `None` outside it.
fn place(f: &Found, width: u32, height: u32) -> Option<Rect> {
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

/// A face's score as a confidence the default threshold (0.7) lets
/// through: [`MIN_SCORE`] -> 0.7, 1.0 -> 1.0. A policy raises it for
/// `FACE` to fill fewer.
pub fn confidence(score: f32) -> f32 {
    let c = 0.7 + 0.3 * (score - MIN_SCORE) / (1.0 - MIN_SCORE);
    (c.clamp(0.0, 1.0) * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_read_from_fitting_up_to_its_own_size() {
        assert_eq!(scales(640, 480), vec![1.0]);
        assert_eq!(scales(320, 200), vec![1.0]);
        assert_eq!(scales(2560, 1440), vec![0.25, 0.5, 1.0]);
        let s = scales(1570, 1157);
        assert_eq!(s.len(), 3);
        assert_eq!(s[2], 1.0);
    }

    #[test]
    fn tiles_cover_a_side_and_overlap() {
        assert_eq!(starts(500), vec![0]);
        assert_eq!(starts(640), vec![0]);
        assert_eq!(starts(1000), vec![0, 360]);
        assert_eq!(starts(1570), vec![0, 480, 930]);
        for len in [641, 1157, 1570, 4000] {
            let s = starts(len);
            assert_eq!(*s.last().unwrap() + SIDE, len);
            for pair in s.windows(2) {
                // Each next tile starts before the one before ends, by at
                // least the overlap.
                assert!(pair[1] + OVERLAP <= pair[0] + SIDE, "{len}: {s:?}");
            }
        }
    }

    #[test]
    fn an_anchor_becomes_a_box_in_the_tile() {
        let cols = SIDE as usize / 32;
        let n = cols * cols;
        let mut cls = vec![0.0; n];
        let mut obj = vec![0.0; n];
        let mut bbox = vec![0.0; 4 * n];
        // Row 2, column 3: centre at (3.5, 2.5) strides, 2 x 4 strides wide.
        let i = 2 * cols + 3;
        cls[i] = 0.81;
        obj[i] = 1.0;
        bbox[4 * i..4 * i + 4].copy_from_slice(&[0.5, 0.5, 2f32.ln(), 4f32.ln()]);
        // A weak one, below the threshold.
        cls[0] = 0.2;
        obj[0] = 0.2;
        let found = decode(32, &cls, &obj, &bbox);
        assert_eq!(found.len(), 1);
        let f = found[0];
        assert!((f.score - 0.9).abs() < 1e-6);
        assert!((f.x - (3.5 * 32.0 - 32.0)).abs() < 1e-3, "{f:?}");
        assert!((f.y - (2.5 * 32.0 - 64.0)).abs() < 1e-3, "{f:?}");
        assert!((f.w - 64.0).abs() < 1e-3 && (f.h - 128.0).abs() < 1e-3);
    }

    #[test]
    fn one_face_found_twice_is_one() {
        let f = |x, y, w, h, score| Found { x, y, w, h, score };
        let faces = distinct(vec![
            f(100.0, 100.0, 60.0, 80.0, 0.8),
            // The same face from a neighbouring anchor and another scale.
            f(102.0, 98.0, 58.0, 84.0, 0.93),
            f(110.0, 110.0, 30.0, 40.0, 0.6),
            // Another face beside it.
            f(300.0, 100.0, 60.0, 80.0, 0.7),
        ]);
        assert_eq!(faces.len(), 2);
        assert_eq!(faces[0].score, 0.93);
        assert_eq!(faces[1].x, 300.0);
    }

    #[test]
    fn a_box_takes_hair_and_ears_and_stays_in_the_picture() {
        let f = Found {
            x: 100.0,
            y: 100.0,
            w: 100.0,
            h: 100.0,
            score: 0.9,
        };
        assert_eq!(
            place(&f, 1000, 1000),
            Some(Rect {
                x: 85,
                y: 85,
                w: 130,
                h: 130
            })
        );
        assert_eq!(
            place(&f, 150, 1000).map(|r| (r.x, r.right())),
            Some((85, 150))
        );
        let left = Found { x: -20.0, ..f };
        assert_eq!(
            place(&left, 1000, 1000).map(|r| (r.x, r.right())),
            Some((0, 95))
        );
        let outside = Found { x: 2000.0, ..f };
        assert_eq!(place(&outside, 1000, 1000), None);
    }

    #[test]
    fn scores_become_confidences_the_default_threshold_keeps() {
        assert_eq!(confidence(MIN_SCORE), 0.7);
        assert_eq!(confidence(1.0), 1.0);
        assert_eq!(confidence(0.75), 0.85);
    }

    /// The real model, when installed: VEILQ_FACE_MODEL, VEILQ_ORT_DYLIB,
    /// and a picture VEILQ_FACE_IMAGE with VEILQ_FACE_EXPECT faces.
    #[test]
    fn the_model_finds_faces() {
        let (Ok(model), Ok(dylib), Ok(image), Ok(expect)) = (
            std::env::var("VEILQ_FACE_MODEL"),
            std::env::var("VEILQ_ORT_DYLIB"),
            std::env::var("VEILQ_FACE_IMAGE"),
            std::env::var("VEILQ_FACE_EXPECT"),
        ) else {
            return;
        };
        let faces = Faces::load(&ModelFiles {
            model: Path::new(&model),
            runtime: Path::new(&dylib),
        })
        .unwrap();
        let img = image::open(image).unwrap().to_rgb8();
        let found = faces.find(&img).unwrap();
        let strong = found.iter().filter(|f| f.score >= 0.7).count();
        assert!(
            strong >= expect.parse::<usize>().unwrap(),
            "{strong} faces: {found:?}"
        );
    }
}
