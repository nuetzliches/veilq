//! The pictures of the corpus (ADR-0023): a screenshot (PNG), a photo of a
//! form with EXIF (JPEG), a screen recording (GIF) and a scanned letter
//! (PDF of one JPEG page). Their text is drawn with Noto Sans; the truth of a
//! value is the box of its ink, in pixels, at the page or frame it is on.
//!
//! A frame of a GIF that is the frame before it again has no truth: veilq
//! does not read it again (its boxes stay), so it has no findings either.

use std::io::Cursor;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use image::codecs::gif::{GifEncoder, Repeat};
use image::codecs::jpeg::JpegEncoder;
use image::{Delay, Frame, Rgb, RgbImage, RgbaImage};
use veilq_core::{EntityType, Location};

use super::{Lookalikes, Person, Rng, Truth, iban_with_zeros};

const FONT: &[u8] = include_bytes!("../../assets/NotoSans-Regular.ttf");

/// A picture being drawn, with the truth of what is drawn into it.
struct Canvas {
    img: RgbImage,
    font: FontRef<'static>,
    frame: Option<u64>,
    truth: Vec<Truth>,
}

/// A box in pixels: x, y, width, height.
type Bounds = (u32, u32, u32, u32);

impl Canvas {
    fn new(w: u32, h: u32, bg: [u8; 3]) -> Self {
        Self {
            img: RgbImage::from_pixel(w, h, Rgb(bg)),
            font: FontRef::try_from_slice(FONT).expect("the font in assets/"),
            frame: None,
            truth: Vec::new(),
        }
    }

    fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: [u8; 3]) {
        for yy in y..(y + h).min(self.img.height()) {
            for xx in x..(x + w).min(self.img.width()) {
                self.img.put_pixel(xx, yy, Rgb(color));
            }
        }
    }

    /// Draws `s` with its top at `y`; the box of its ink and where it ends.
    fn text(&mut self, x: f32, y: f32, size: f32, s: &str, color: [u8; 3]) -> (Bounds, f32) {
        let scale = PxScale::from(size);
        let scaled = self.font.as_scaled(scale);
        let baseline = y + scaled.ascent();
        let mut caret = x;
        let mut prev = None;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let (w, h) = self.img.dimensions();
        for c in s.chars() {
            let id = self.font.glyph_id(c);
            if let Some(p) = prev {
                caret += scaled.kern(p, id);
            }
            let glyph = id.with_scale_and_position(scale, point(caret, baseline));
            caret += scaled.h_advance(id);
            prev = Some(id);
            let Some(outline) = self.font.outline_glyph(glyph) else {
                continue;
            };
            let b = outline.px_bounds();
            outline.draw(|gx, gy, cov| {
                let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                if px < 0 || py < 0 || px as u32 >= w || py as u32 >= h || cov <= 0.0 {
                    return;
                }
                let p = self.img.get_pixel_mut(px as u32, py as u32);
                for (v, c) in p.0.iter_mut().zip(color) {
                    *v = (*v as f32 * (1.0 - cov) + c as f32 * cov).round() as u8;
                }
            });
            x0 = x0.min(b.min.x);
            y0 = y0.min(b.min.y);
            x1 = x1.max(b.max.x);
            y1 = y1.max(b.max.y);
        }
        let bounds = (
            x0.max(0.0) as u32,
            y0.max(0.0) as u32,
            (x1 - x0).ceil().max(0.0) as u32,
            (y1 - y0).ceil().max(0.0) as u32,
        );
        (bounds, caret)
    }

    /// How wide `s` is drawn at `size`.
    fn width(&self, size: f32, s: &str) -> f32 {
        let scaled = self.font.as_scaled(PxScale::from(size));
        let mut prev = None;
        let mut w = 0.0;
        for c in s.chars() {
            let id = self.font.glyph_id(c);
            if let Some(p) = prev {
                w += scaled.kern(p, id);
            }
            w += scaled.h_advance(id);
            prev = Some(id);
        }
        w
    }

    /// `s` as a table shows it in a column `w` wide: cut short with ` ...`
    /// when it does not fit.
    fn fit(&self, size: f32, s: &str, w: f32) -> String {
        if self.width(size, s) <= w {
            return s.to_owned();
        }
        let mut chars: Vec<char> = s.chars().collect();
        loop {
            chars.pop();
            let cut: String = chars.iter().collect::<String>().trim_end().to_owned() + " ...";
            if chars.is_empty() || self.width(size, &cut) <= w {
                return cut;
            }
        }
    }

    /// Text that is no personal data; where it ends.
    fn lit(&mut self, x: f32, y: f32, size: f32, s: &str, color: [u8; 3]) -> f32 {
        self.text(x, y, size, s, color).1
    }

    /// Personal data: drawn, and its ink in the truth; where it ends.
    fn pii(&mut self, entity: &str, x: f32, y: f32, size: f32, s: &str, color: [u8; 3]) -> f32 {
        let ((bx, by, bw, bh), end) = self.text(x, y, size, s, color);
        self.truth.push(Truth {
            entity: EntityType::new(entity).expect("a valid type"),
            location: Location::Region {
                part: None,
                frame: self.frame,
                x: bx,
                y: by,
                width: bw,
                height: bh,
            },
        });
        end
    }
}

const INK: [u8; 3] = [30, 34, 44];
const MUTED: [u8; 3] = [110, 117, 134];

/// The rows of a customer's record: label and value, the value personal data.
fn record_rows(r: &mut Rng, p: &Person) -> Vec<(&'static str, &'static str, String)> {
    vec![
        ("Geburtsdatum", "DATE_OF_BIRTH", p.dob_de(r)),
        ("Anschrift", "ADDRESS", format!("{}, {}", p.street, p.city)),
        ("Telefon", "PHONE_NUMBER", p.phone.clone()),
        ("E-Mail", "EMAIL_ADDRESS", p.email.clone()),
        ("IBAN", "IBAN_CODE", p.iban_spaced()),
        ("Steuer-ID", "TAX_ID", p.tax_id_spaced()),
        ("Kfz", "LICENSE_PLATE", p.plate.clone()),
    ]
}

/// The frame of an application: a top bar with the user logged in, a menu.
fn app_frame(c: &mut Canvas, user: &Person) {
    let w = c.img.width();
    c.rect(0, 0, w, 44, [42, 54, 79]);
    c.lit(
        16.0,
        10.0,
        18.0,
        "Kundenverwaltung – Beispiel GmbH",
        [255, 255, 255],
    );
    let x = c.lit(
        w as f32 - 290.0,
        14.0,
        14.0,
        "angemeldet: ",
        [200, 206, 220],
    );
    c.pii("PERSON", x, 14.0, 14.0, &user.name(), [255, 255, 255]);
    c.rect(0, 44, 170, c.img.height() - 44, [229, 232, 240]);
    for (i, item) in ["Übersicht", "Kunden", "Verträge", "Zahlungen"]
        .iter()
        .enumerate()
    {
        c.lit(18.0, 62.0 + 30.0 * i as f32, 15.0, item, INK);
    }
}

/// A customer's record in the application.
fn record(c: &mut Canvas, r: &mut Rng, n: u64, p: &Person, x: &Lookalikes) {
    let rows = record_rows(r, p);
    record_of(c, n, p, &rows, x);
}

/// A customer's record with the rows given, so it can be drawn again alike.
fn record_of(
    c: &mut Canvas,
    n: u64,
    p: &Person,
    rows: &[(&'static str, &'static str, String)],
    x: &Lookalikes,
) {
    let end = c.lit(196.0, 60.0, 19.0, &format!("Kunde #{n} – "), INK);
    c.pii("PERSON", end, 60.0, 19.0, &p.name(), INK);
    for (i, (label, entity, value)) in rows.iter().enumerate() {
        let y = 104.0 + 32.0 * i as f32;
        c.lit(210.0, y, 15.0, label, MUTED);
        c.pii(entity, 360.0, y, 15.0, value, INK);
    }
    let h = c.img.height() as f32;
    c.lit(
        210.0,
        h - 40.0,
        13.0,
        &format!("Vorgang {} · Version {}", x.invoice, x.version),
        MUTED,
    );
}

/// A list of customers: name and mail.
fn list(c: &mut Canvas, people: &[Person]) {
    c.lit(196.0, 60.0, 19.0, "Kunden", INK);
    for (i, p) in people.iter().enumerate() {
        let y = 104.0 + 36.0 * i as f32;
        c.rect(196, y as u32 - 6, 680, 32, [255, 255, 255]);
        c.pii("PERSON", 210.0, y, 15.0, &p.name(), INK);
        c.pii("EMAIL_ADDRESS", 470.0, y, 15.0, &p.email, INK);
    }
}

/// A mouse pointer at `x`, `y`.
fn pointer(c: &mut Canvas, x: u32, y: u32) {
    for row in 0..16 {
        c.rect(x, y + row, row / 2 + 1, 1, [0, 0, 0]);
    }
}

fn png(img: &RgbImage) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .expect("a PNG");
    out.into_inner()
}

fn jpeg(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 90)
        .encode_image(img)
        .expect("a JPEG");
    out
}

/// A screenshot of a customer's record.
pub(super) fn screenshot_png(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let (user, p, x) = (Person::de(r), Person::de(r), Lookalikes::new(r));
    let mut c = Canvas::new(900, 400, [244, 245, 248]);
    app_frame(&mut c, &user);
    let n = 1 + r.below(900);
    record(&mut c, r, n, &p, &x);
    (png(&c.img), c.truth)
}

/// A photo of a paper form, with EXIF (camera, time): the metadata is a
/// finding of its own, the whole picture.
pub(super) fn form_jpeg(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let (p, x) = (Person::de(r), Lookalikes::new(r));
    let mut c = Canvas::new(820, 460, [236, 233, 225]);
    c.lit(40.0, 30.0, 22.0, "Antrag auf Erstattung", INK);
    c.lit(
        40.0,
        66.0,
        14.0,
        &format!("Vorgang {} vom {}", x.invoice, x.date),
        MUTED,
    );
    let end = c.lit(40.0, 104.0, 16.0, "Name: ", MUTED);
    c.pii("PERSON", end + 4.0, 104.0, 16.0, &p.name(), INK);
    for (i, (label, entity, value)) in record_rows(r, &p).into_iter().enumerate() {
        let y = 140.0 + 34.0 * i as f32;
        c.lit(40.0, y, 16.0, label, MUTED);
        c.pii(entity, 220.0, y, 16.0, &value, INK);
    }
    let (w, h) = c.img.dimensions();
    let mut truth = vec![Truth {
        entity: EntityType::new("METADATA").expect("a valid type"),
        location: Location::Region {
            part: None,
            frame: None,
            x: 0,
            y: 0,
            width: w,
            height: h,
        },
    }];
    truth.extend(c.truth);
    (with_exif(&jpeg(&c.img)), truth)
}

/// A screenshot of a transfer form: the sender's account in a drop-down,
/// small, under a compound label (`Senderkonto`), its IBAN unspaced with a
/// run of zeros, and the payee. At 11 px OCR reads the zeros one short; the
/// IBAN is redacted all the same (ADR-0037).
pub(super) fn transfer_png(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let (p, x) = (Person::de(r), Lookalikes::new(r));
    let iban = iban_with_zeros(r);
    let mut c = Canvas::new(760, 300, [255, 255, 255]);
    c.lit(20.0, 16.0, 15.0, "Neue Überweisung erstellen", INK);
    c.lit(20.0, 60.0, 11.0, "FÄLLIGKEIT", MUTED);
    c.lit(200.0, 56.0, 12.0, "Fälligkeit *", INK);
    c.lit(
        200.0,
        76.0,
        12.0,
        &format!("{} {} €", x.date, x.amount),
        INK,
    );
    c.lit(20.0, 124.0, 11.0, "VON", MUTED);
    c.lit(200.0, 120.0, 12.0, "Senderkonto *", INK);
    let end = c.lit(200.0, 140.0, 11.0, "Musterbank - ", INK);
    let end = c.pii("IBAN_CODE", end, 140.0, 11.0, &iban, INK);
    c.lit(end, 140.0, 11.0, " (Beispiel Handel GmbH)", INK);
    c.lit(20.0, 188.0, 11.0, "AN", MUTED);
    c.lit(200.0, 184.0, 12.0, "Empfänger *", INK);
    c.pii("PERSON", 200.0, 204.0, 12.0, &p.name(), INK);
    c.lit(200.0, 250.0, 12.0, "Verwendungszweck *", INK);
    c.lit(200.0, 270.0, 12.0, &format!("Rechnung {}", x.invoice), INK);
    (png(&c.img), c.truth)
}

/// Streets of a case list, also without a street word.
const LIST_STREETS: &[&str] = &[
    "Eschenbreite",
    "Im Kleefeld",
    "Am Mühlenbach",
    "Hohenauer Straße",
    "Birkenhainer Str.",
    "Wiesengrund",
    "Kastanienallee",
    "Rosenhöhe",
];

/// What a case list's status column says.
const LIST_STATUS: &[&str] = &[
    "",
    "",
    "Unterlagen angefordert",
    "Klärung nach Gespräch",
    "Antwort ausstehend",
];

/// A case list in a browser (ADR-0041, ADR-0042): the client, a surname
/// alone in the column of opposing parties, a status, and street, number,
/// postcode and town in columns of their own, cells cut short where a
/// value is too long for its column.
pub(super) fn list_png(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    const ROW: f32 = 30.0;
    // Title, x, width.
    let cols: [(&str, f32, f32); 10] = [
        ("Kürzel", 10.0, 50.0),
        ("SB", 70.0, 40.0),
        ("Akte", 120.0, 80.0),
        ("Mandant", 210.0, 140.0),
        ("Gegner", 360.0, 110.0),
        ("Status", 480.0, 200.0),
        ("Straße", 690.0, 84.0),
        ("Nr.", 780.0, 40.0),
        ("PLZ", 830.0, 60.0),
        ("Ort", 900.0, 70.0),
    ];
    let rows = 6;
    let mut c = Canvas::new(1020, 50 + rows * ROW as u32, [255, 255, 255]);
    for (title, x, _) in cols {
        c.lit(x, 12.0, 12.0, title, INK);
    }
    c.rect(0, 34, 1020, 1, [210, 213, 220]);
    for i in 0..rows {
        let y = 44.0 + i as f32 * ROW;
        let (p, other) = (Person::de(r), Person::de(r));
        let (plz, town) = p.city.split_once(' ').expect("a postcode and a town");
        let street = r.pick(LIST_STREETS);
        let number = format!(
            "{}{}",
            1 + r.below(120),
            if r.chance(15) { "c" } else { "" }
        );
        c.lit(cols[0].1, y, 12.0, r.pick(&["MTH", "CLI", "LKA"]), INK);
        c.lit(cols[1].1, y, 12.0, "sab", INK);
        c.lit(cols[2].1, y, 12.0, &format!("B0{}", r.digits(4)), INK);
        if r.chance(25) {
            c.lit(cols[3].1, y, 12.0, "Nordwerk GmbH", INK);
        } else {
            c.pii("PERSON", cols[3].1, y, 12.0, &p.name(), INK);
        }
        if r.chance(60) {
            c.pii("PERSON", cols[4].1, y, 12.0, &other.last, INK);
        }
        let status = r.pick(LIST_STATUS);
        if !status.is_empty() {
            c.lit(cols[5].1, y, 12.0, status, INK);
        }
        for (k, value) in [(6, street), (7, number.as_str()), (8, plz), (9, town)] {
            let (_, x, w) = cols[k];
            let shown = c.fit(12.0, value, w - 8.0);
            c.pii("ADDRESS", x, y, 12.0, &shown, INK);
        }
        c.rect(0, (y + 22.0) as u32, 1020, 1, [232, 234, 238]);
    }
    (png(&c.img), c.truth)
}

/// `jpeg` with an EXIF segment: a camera's make and model.
fn with_exif(jpeg: &[u8]) -> Vec<u8> {
    // TIFF, little endian, one IFD with Make (0x010f) as an ASCII string.
    let make = b"Beispiel Kamera\0";
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II*\0");
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x010fu16.to_le_bytes());
    tiff.extend_from_slice(&2u16.to_le_bytes());
    tiff.extend_from_slice(&(make.len() as u32).to_le_bytes());
    tiff.extend_from_slice(&26u32.to_le_bytes());
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(make);
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&tiff);
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xff, 0xe1]);
    out.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// A screen recording: the list, a record, the list again with the pointer
/// moved, and that frame once more.
pub(super) fn recording_gif(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let user = Person::de(r);
    let people: Vec<Person> = (0..3).map(|_| Person::de(r)).collect();
    let x = Lookalikes::new(r);
    let mut truth = Vec::new();
    let mut frames = Vec::new();
    let draw = |n: u64, r: &mut Rng, kind: u8, truth: &mut Vec<Truth>| {
        let mut c = Canvas::new(900, 400, [244, 245, 248]);
        c.frame = Some(n);
        app_frame(&mut c, &user);
        match kind {
            0 => {
                list(&mut c, &people);
                pointer(&mut c, 300, 270);
            }
            1 => record(&mut c, r, 1, &people[0], &x),
            _ => {
                list(&mut c, &people);
                pointer(&mut c, 420, 300);
            }
        }
        truth.extend(c.truth);
        c.img
    };
    for (n, kind) in [(1, 0), (2, 1), (3, 2)] {
        frames.push(draw(n, r, kind, &mut truth));
    }
    frames.push(frames[2].clone());
    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new_with_speed(&mut out, 10);
        encoder.set_repeat(Repeat::Infinite).expect("a GIF");
        for img in frames {
            let rgba: RgbaImage = image::DynamicImage::ImageRgb8(img).to_rgba8();
            encoder
                .encode_frame(Frame::from_parts(
                    rgba,
                    0,
                    0,
                    Delay::from_numer_denom_ms(800, 1),
                ))
                .expect("a GIF frame");
        }
    }
    (out, truth)
}

/// A state of the long recording's screen.
enum Screen {
    /// The list of customers, scrolled down by this many rows.
    List(usize),
    /// The first customer's record: how much of the note is typed, whether
    /// a second mail address is pasted, whether the phone number is new.
    Record {
        typed: usize,
        mail: bool,
        phone: bool,
    },
}

/// A long screen recording at 1920x1080, as a recorder writes it: one
/// palette, so what stands still keeps its pixels. A pointer moves, a clock
/// in the top bar ticks, a list scrolls, a record opens, a note is typed, a
/// mail address is pasted and a phone number replaced. A value is in the
/// truth of a frame only where its ink changed against the frame before:
/// where it stands still, its box from the frame before stays.
pub(super) fn long_recording_gif(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let user = Person::de(r);
    let people: Vec<Person> = (0..40).map(|_| Person::de(r)).collect();
    let x = Lookalikes::new(r);
    let rows = record_rows(r, &people[0]);
    let mut changed_rows = rows.clone();
    changed_rows[2].2 = Person::de(r).phone;
    let mail = Person::de(r).email;
    let note = "Rückruf vereinbart, Unterlagen folgen per Post";
    let mut script: Vec<(Screen, (u32, u32))> = Vec::new();
    for i in 0..30 {
        script.push((Screen::List(0), (600 + 24 * i, 300 + 9 * i)));
    }
    for s in 1..=4 {
        script.push((Screen::List(3 * s), (1300, 570)));
    }
    let record = |typed, mail, phone| Screen::Record { typed, mail, phone };
    for typed in 0..=note.chars().count() {
        script.push((record(typed, false, false), (900, 470)));
    }
    let all = note.chars().count();
    for i in 0..20 {
        script.push((record(all, true, false), (900 + 30 * i, 520)));
    }
    for i in 0..20 {
        script.push((record(all, true, true), (1500, 520 + 20 * i)));
    }
    let mut truth = Vec::new();
    let mut prev: Option<RgbImage> = None;
    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new_with_speed(&mut out, 10);
        encoder.set_repeat(Repeat::Infinite).expect("a GIF");
        for (i, (screen, (px, py))) in script.into_iter().enumerate() {
            let n = i as u64 + 1;
            let mut c = Canvas::new(1920, 1080, [244, 245, 248]);
            c.frame = Some(n);
            app_frame(&mut c, &user);
            c.lit(
                960.0,
                14.0,
                14.0,
                &format!("Sitzung 00:{:02}:{:02}", i / 60, i % 60),
                [200, 206, 220],
            );
            match screen {
                Screen::List(skip) => list(&mut c, &people[skip..skip + 24]),
                Screen::Record {
                    typed,
                    mail: pasted,
                    phone,
                } => {
                    record_of(
                        &mut c,
                        1,
                        &people[0],
                        if phone { &changed_rows } else { &rows },
                        &x,
                    );
                    c.lit(210.0, 400.0, 15.0, "Notiz", MUTED);
                    let typed: String = note.chars().take(typed).collect();
                    c.lit(360.0, 400.0, 15.0, &typed, INK);
                    if pasted {
                        c.lit(210.0, 432.0, 15.0, "E-Mail privat", MUTED);
                        c.pii("EMAIL_ADDRESS", 360.0, 432.0, 15.0, &mail, INK);
                    }
                }
            }
            pointer(&mut c, px, py);
            // One palette: 6 levels a channel, 216 colours, so the encoder
            // keeps every pixel as it is.
            for p in c.img.pixels_mut() {
                for v in p.0.iter_mut() {
                    *v = ((u32::from(*v) + 25) / 51 * 51) as u8;
                }
            }
            let fresh = |t: &Truth| match (&prev, &t.location) {
                (
                    Some(before),
                    Location::Region {
                        x,
                        y,
                        width,
                        height,
                        ..
                    },
                ) => (*y..y + height).any(|yy| {
                    (*x..x + width).any(|xx| before.get_pixel(xx, yy) != c.img.get_pixel(xx, yy))
                }),
                _ => true,
            };
            truth.extend(c.truth.iter().filter(|t| fresh(t)).cloned());
            let rgba: RgbaImage = image::DynamicImage::ImageRgb8(c.img.clone()).to_rgba8();
            encoder
                .encode_frame(Frame::from_parts(
                    rgba,
                    0,
                    0,
                    Delay::from_numer_denom_ms(100, 1),
                ))
                .expect("a GIF frame");
            prev = Some(c.img);
        }
    }
    (out, truth)
}

/// A scanned letter: one page, a JPEG at 200 dpi in a PDF with no text.
pub(super) fn scan_pdf(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let (p, q, x) = (Person::de(r), Person::de(r), Lookalikes::new(r));
    let mut c = Canvas::new(1240, 900, [252, 252, 250]);
    c.frame = Some(1);
    c.lit(
        80.0,
        70.0,
        20.0,
        "Beispiel GmbH · Musterstraße 1 · 28195 Bremen",
        MUTED,
    );
    c.pii("PERSON", 80.0, 150.0, 26.0, &p.name(), INK);
    c.pii(
        "ADDRESS",
        80.0,
        192.0,
        26.0,
        &format!("{}, {}", p.street, p.city),
        INK,
    );
    c.lit(
        80.0,
        290.0,
        26.0,
        &format!("Rechnung {} vom {}", x.invoice, x.date),
        INK,
    );
    let end = c.lit(
        80.0,
        380.0,
        26.0,
        &format!("Bitte überweisen Sie {} EUR auf IBAN ", x.amount),
        INK,
    );
    c.pii("IBAN_CODE", end, 380.0, 26.0, &p.iban_spaced(), INK);
    let end = c.lit(80.0, 430.0, 26.0, "Rückfragen: ", INK);
    let end = c.pii("PERSON", end, 430.0, 26.0, &q.name(), INK);
    let end = c.lit(end, 430.0, 26.0, ", ", INK);
    c.pii("PHONE_NUMBER", end, 430.0, 26.0, &q.phone, INK);
    let end = c.lit(80.0, 520.0, 26.0, "Ihre Steuer-ID: ", INK);
    let end = c.pii("TAX_ID", end, 520.0, 26.0, &p.tax_id_spaced(), INK);
    let end = c.lit(end, 520.0, 26.0, " · Kennzeichen ", INK);
    c.pii("LICENSE_PLATE", end, 520.0, 26.0, &p.plate, INK);
    let (w, h) = c.img.dimensions();
    (pdf_of_jpeg(&jpeg(&c.img), w, h), c.truth)
}

/// A one-page PDF that is the JPEG `data` (`w` x `h` pixels at 200 dpi).
fn pdf_of_jpeg(data: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (pw, ph) = (w as f32 * 72.0 / 200.0, h as f32 * 72.0 / 200.0);
    let content = format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /Im0 Do Q");
    let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    object(
        &mut out,
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>"
        )
        .as_bytes(),
    );
    let mut image = format!(
        "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
        data.len()
    )
    .into_bytes();
    image.extend_from_slice(data);
    image.extend_from_slice(b"\nendstream");
    object(&mut out, &image);
    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend_from_slice(content.as_bytes());
    stream.extend_from_slice(b"\nendstream");
    object(&mut out, &stream);
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    out
}
