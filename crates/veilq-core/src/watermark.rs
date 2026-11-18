//! The mark a written picture carries (ADR-0050): `veilq <version>` at its
//! bottom right, light letters on a dark translucent box.
//!
//! It is drawn onto the output only, after a picture was redacted and its
//! verify rounds settled: OCR and the detectors never read it. The letters
//! come from a 5x7 bitmap table here, so no font is shipped.

use image::{Rgba, RgbaImage};

/// Columns and rows of a glyph, before scaling.
const GLYPH_W: u32 = 5;
const GLYPH_H: u32 = 7;
/// A glyph and the gap after it.
const ADVANCE: u32 = GLYPH_W + 1;
/// The box's padding around the text and its distance from the picture's
/// edges, in glyph pixels.
const PAD: u32 = 2;
/// The box's opacity, of 255.
const BOX_ALPHA: u32 = 150;
const INK: [u8; 3] = [240, 240, 240];

/// What the mark says: this build's name and version.
pub fn text() -> String {
    format!("veilq {}", env!("CARGO_PKG_VERSION"))
}

/// The scale for a picture of `width` x `height`: one glyph pixel per
/// 500 pixels of its shorter side, at least one.
pub(crate) fn scale_for(width: u32, height: u32) -> u32 {
    (width.min(height) / 500).max(1)
}

/// Draws the mark onto `img` at `scale`, `margin` pixels from its bottom
/// and right edges. A picture too small for it (the box would take more
/// than half its width or a quarter of its height) is left as it is.
pub(crate) fn stamp(img: &mut RgbaImage, scale: u32, margin: u32) {
    let text = text();
    let chars = text.chars().count() as u32;
    let pad = PAD * scale;
    let box_w = (chars * ADVANCE - 1) * scale + 2 * pad;
    let box_h = GLYPH_H * scale + 2 * pad;
    let (w, h) = img.dimensions();
    if 2 * box_w > w || 4 * box_h > h || box_w + margin > w || box_h + margin > h {
        return;
    }
    let (x0, y0) = (w - margin - box_w, h - margin - box_h);
    for y in y0..y0 + box_h {
        for x in x0..x0 + box_w {
            let p = img.get_pixel_mut(x, y);
            for v in &mut p.0[..3] {
                *v = (u32::from(*v) * (255 - BOX_ALPHA) / 255) as u8;
            }
            p.0[3] = p.0[3].max(BOX_ALPHA as u8);
        }
    }
    for (i, c) in text.chars().enumerate() {
        let rows = glyph(c);
        let gx = x0 + pad + i as u32 * ADVANCE * scale;
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..GLYPH_W {
                if bits & (1 << (GLYPH_W - 1 - col)) == 0 {
                    continue;
                }
                let (px, py) = (gx + col * scale, y0 + pad + row as u32 * scale);
                for dy in 0..scale {
                    for dx in 0..scale {
                        img.put_pixel(px + dx, py + dy, Rgba([INK[0], INK[1], INK[2], 255]));
                    }
                }
            }
        }
    }
}

/// The rows of `c`, top first, five bits each (the highest bit is the
/// leftmost column). A character without a glyph is a box, so a version
/// string the table lacks shows, rather than vanishes.
fn glyph(c: char) -> [u8; 7] {
    match c {
        ' ' => [0; 7],
        '.' => [0, 0, 0, 0, 0, 0b01100, 0b01100],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        '+' => [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        'a' => [0, 0, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111],
        'b' => [
            0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b11110,
        ],
        'c' => [0, 0, 0b01110, 0b10000, 0b10000, 0b10001, 0b01110],
        'd' => [
            0b00001, 0b00001, 0b01101, 0b10011, 0b10001, 0b10001, 0b01111,
        ],
        'e' => [0, 0, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110],
        'f' => [
            0b00110, 0b01001, 0b01000, 0b11100, 0b01000, 0b01000, 0b01000,
        ],
        'g' => [0, 0b01111, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110],
        'h' => [
            0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001,
        ],
        'i' => [0b00100, 0, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110],
        'j' => [0b00010, 0, 0b00110, 0b00010, 0b00010, 0b10010, 0b01100],
        'k' => [
            0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010,
        ],
        'l' => [
            0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'm' => [0, 0, 0b11010, 0b10101, 0b10101, 0b10001, 0b10001],
        'n' => [0, 0, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001],
        'o' => [0, 0, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110],
        'p' => [0, 0, 0b11110, 0b10001, 0b11110, 0b10000, 0b10000],
        'q' => [0, 0, 0b01111, 0b10001, 0b01111, 0b00001, 0b00001],
        'r' => [0, 0, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000],
        's' => [0, 0, 0b01110, 0b10000, 0b01110, 0b00001, 0b11110],
        't' => [
            0b01000, 0b01000, 0b11100, 0b01000, 0b01000, 0b01001, 0b00110,
        ],
        'u' => [0, 0, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101],
        'v' => [0, 0, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'w' => [0, 0, 0b10001, 0b10001, 0b10101, 0b10101, 0b01010],
        'x' => [0, 0, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001],
        'y' => [0, 0, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110],
        'z' => [0, 0, 0b11111, 0b00010, 0b00100, 0b01000, 0b11111],
        _ => [
            0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);

    #[test]
    fn the_mark_names_veilq_and_this_version() {
        assert_eq!(text(), format!("veilq {}", env!("CARGO_PKG_VERSION")));
        // Every character a version can hold has its own glyph.
        let unknown = glyph('\u{1}');
        for c in "veilq 0123456789.-+abcdefghijklmnopqrstuvwxyz".chars() {
            if c != ' ' {
                assert_ne!(glyph(c), unknown, "{c:?}");
            }
        }
    }

    #[test]
    fn only_the_bottom_right_box_changes() {
        let mut img = RgbaImage::from_pixel(800, 600, WHITE);
        stamp(&mut img, 1, 4);
        let changed: Vec<(u32, u32)> = img
            .enumerate_pixels()
            .filter(|(_, _, p)| **p != WHITE)
            .map(|(x, y, _)| (x, y))
            .collect();
        assert!(!changed.is_empty());
        let chars = text().chars().count() as u32;
        let (box_w, box_h) = ((chars * ADVANCE - 1) + 2 * PAD, GLYPH_H + 2 * PAD);
        for (x, y) in changed {
            assert!(x >= 800 - 4 - box_w && x < 800 - 4, "{x}");
            assert!(y >= 600 - 4 - box_h && y < 600 - 4, "{y}");
        }
        // Light letters on a darker box.
        assert!(img.pixels().any(|p| p.0[..3] == INK));
        assert!(img.pixels().any(|p| p.0[0] < 200));
    }

    #[test]
    fn a_small_picture_gets_no_mark() {
        let mut img = RgbaImage::from_pixel(60, 30, WHITE);
        stamp(&mut img, 1, 2);
        assert!(img.pixels().all(|p| *p == WHITE));
    }

    #[test]
    fn the_scale_grows_with_the_picture() {
        assert_eq!(scale_for(100, 80), 1);
        assert_eq!(scale_for(1920, 1080), 2);
        assert_eq!(scale_for(3840, 2160), 4);
    }
}
