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
/// The hand that fills a form in (#59).
const HAND: &[u8] = include_bytes!("../../assets/IndieFlower-Regular.ttf");

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

    /// Writes by hand from now on (`true`), or prints again.
    fn hand(&mut self, on: bool) {
        self.font =
            FontRef::try_from_slice(if on { HAND } else { FONT }).expect("a font in assets/");
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

/// A news feed of three cards side by side: headlines set with room between
/// their lines, one broken after a person's given name (its surname starts
/// the line below), a kin word where a name could stand (`kann Vater nicht
/// vergeben`), and a book whose title names a saint. Only the person is
/// personal data.
pub(super) fn news_png(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let (p, q) = (Person::de(r), Person::de(r));
    let mut c = Canvas::new(1000, 230, [40, 40, 44]);
    let white = [250, 250, 250];
    let grey = [180, 180, 180];
    let size = 19.0;
    let gap = 31.0; // a headline's lines: more than a cell's half a line apart
    c.lit(20.0, 20.0, 13.0, "Nordblatt · 9 Std.", grey);
    c.lit(20.0, 50.0, size, "Neue Brücke am Hafen", white);
    c.lit(20.0, 50.0 + gap, size, "wird im Mai eröffnet", white);
    c.lit(350.0, 20.0, 13.0, "Abendpost · 9 Std.", grey);
    let end = c.lit(350.0, 50.0, size, "Jetzt auch noch ", white);
    c.pii("PERSON", end, 50.0, size, &p.first, white);
    let end = c.pii("PERSON", 350.0, 50.0 + gap, size, &p.last, white);
    c.lit(end, 50.0 + gap, size, "!: Wenn alle deine", white);
    c.lit(350.0, 50.0 + 2.0 * gap, size, "Lieblinge von früher", white);
    c.lit(690.0, 20.0, 13.0, "Stadtbote · 2 Std.", grey);
    let end = c.lit(690.0, 50.0, size, "Nach dem Streit: ", white);
    c.pii("PERSON", end, 50.0, size, &q.last, white);
    c.lit(690.0, 50.0 + gap, size, "kann Vater nicht vergeben", white);
    c.lit(
        690.0,
        50.0 + 2.0 * gap,
        size,
        "Die Geschichte von Sankt Martin",
        white,
    );
    (png(&c.img), c.truth)
}

/// Brands of the UI screenshots, made up: short capitalised words that are
/// no person's, as a device maker's in an app's title (#169).
const BRANDS: &[&str] = &[
    "Corvex", "Nordwell", "Kestrel", "Brava", "Lumio", "Tavon", "Orbis", "Velora",
];
/// What follows a brand in an app's title.
const APPS: &[&str] = &["Direct", "Hub", "Connect", "Center", "Studio"];
/// A model's series.
const SERIES: &[&str] = &["Evo", "Link", "Pro", "Air", "Studio", "Go"];
/// A settings window's side bar.
const MENU: &[&str] = &[
    "Startseite",
    "System",
    "Bluetooth und Geräte",
    "Netzwerk und Internet",
    "Personalisierung",
    "Apps",
    "Konten",
    "Zeit und Sprache",
    "Datenschutz und Sicherheit",
    "Updates",
    "Barrierefreiheit",
    "Speicher",
];
/// A setting's label and its description.
const SETTINGS: &[(&str, &str)] = &[
    (
        "Ruhezustand",
        "Wählen Sie, wann sich das Gerät nach einer Pause abschaltet.",
    ),
    (
        "Signaltöne",
        "Ist es eingeschaltet, hören Sie Töne bei jeder Aktion.",
    ),
    (
        "Statuslicht",
        "Ist es eingeschaltet, leuchtet das Licht während eines Anrufs.",
    ),
    (
        "Gerätename",
        "Unter diesem Namen erscheint das Gerät beim Koppeln.",
    ),
    (
        "Erinnerungston",
        "Ein Ton erinnert daran, dass das Mikrofon stumm ist.",
    ),
    (
        "Automatische Updates",
        "Neue Versionen werden nachts installiert.",
    ),
    (
        "Energiesparen",
        "Die Helligkeit sinkt, wenn der Akku schwach ist.",
    ),
];
/// What a device's panel says under its name.
const STATES: &[&str] = &[
    "Verbunden",
    "Aktiviert",
    "Standardgerät",
    "Nicht verbunden",
    "Bereit",
];

/// `c` with each channel moved by `d`.
fn shade(c: [u8; 3], d: i32) -> [u8; 3] {
    c.map(|v| (i32::from(v) + d).clamp(0, 255) as u8)
}

/// The colour `k` percent of the way from `a` to `b`.
fn toward(a: [u8; 3], b: [u8; 3], k: u32) -> [u8; 3] {
    let mut out = a;
    for i in 0..3 {
        let (a, b) = (i32::from(a[i]), i32::from(b[i]));
        out[i] = (a + (b - a) * k as i32 / 100) as u8;
    }
    out
}

/// A made-up brand's model: `Corvex Evo 45`.
fn model(r: &mut Rng, brand: &str) -> String {
    format!("{brand} {} {}", r.pick(SERIES), 10 + r.below(90))
}

/// A screenshot of one to three app windows side by side (#169), drawn
/// with spread in what decided past cases: light or dark, panels 5 to 60
/// off their window, pale text beside normal text, screen scaling, brands
/// and product names beside persons' data, and the picture cut at its
/// bottom (and at times its top) through whatever stands there. Only the
/// persons' data are truth; what the cut leaves of a value is its truth.
pub(super) fn ui_png(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let scale = [100, 125, 150, 175, 200][r.below(5) as usize] as f32 / 100.0;
    let px = |v: f32| v * scale;
    let dark = r.chance(50);
    let base = if dark {
        let v = 20 + r.below(50) as u8;
        [v, v + r.below(8) as u8, v + r.below(14) as u8]
    } else {
        let v = 225 + r.below(30) as u8;
        [v, v, v - r.below(6) as u8]
    };
    let ink = if dark { [232, 232, 236] } else { [26, 28, 34] };
    let windows = 1 + r.below(3) as u32;
    let ww = (px(300.0 + r.below(260) as f32)) as u32;
    let (w, full) = (ww * windows, px(380.0 + r.below(260) as f32) as u32);
    let mut c = Canvas::new(w, full, base);
    let person = Person::de(r);
    // The window that shows the account; another may name a phone after
    // a second person.
    let account = r.below(u64::from(windows)) as u32;
    let owner = Person::de(r);
    for i in 0..windows {
        let x0 = i * ww;
        let off = if i == 0 { 0 } else { (r.below(81) as i32) - 40 };
        let bg = shade(base, off);
        c.rect(x0, 0, ww, full, bg);
        let pale = toward(bg, ink, 35 + r.below(35) as u32);
        let muted = toward(bg, ink, 55 + r.below(25) as u32);
        let panel = shade(bg, (5 + r.below(56) as i32) * if dark { 1 } else { -1 });
        let brand = r.pick(BRANDS).to_owned();
        let x = x0 as f32 + px(16.0);
        let title = if r.chance(70) {
            format!("{brand} {}", r.pick(APPS))
        } else {
            "Einstellungen".to_owned()
        };
        c.lit(x, px(6.0), px(12.0), &title, muted);
        let mut y = px(34.0);
        if i == account {
            // An account: a picture, the name, the e-mail address, at times
            // the phone number; then the side bar.
            c.rect(
                x as u32,
                y as u32,
                px(44.0) as u32,
                px(44.0) as u32,
                shade(bg, 70),
            );
            let tx = x + px(56.0);
            c.pii("PERSON", tx, y + px(2.0), px(15.0), &person.name(), ink);
            c.pii(
                "EMAIL_ADDRESS",
                tx,
                y + px(22.0),
                px(12.0),
                &person.email,
                muted,
            );
            y += px(64.0);
            if r.chance(40) {
                let end = c.lit(x, y, px(12.0), "Telefon: ", muted);
                c.pii("PHONE_NUMBER", end, y, px(12.0), &person.phone, muted);
                y += px(26.0);
            }
            while y < full as f32 {
                c.lit(x, y, px(14.0), r.pick(MENU), ink);
                y += px(34.0 + r.below(10) as f32);
            }
        } else if r.chance(50) {
            // Settings: a heading, then labels with their descriptions, some
            // in pale text, and a field with the device's model.
            c.lit(x, y, px(22.0), "Geräteeinstellungen", ink);
            y += px(44.0);
            while y < full as f32 {
                let (label, text) = SETTINGS[r.below(SETTINGS.len() as u64) as usize];
                c.lit(x, y, px(14.0), label, ink);
                let colour = if r.chance(30) { pale } else { muted };
                c.lit(x + px(150.0), y, px(12.0), text, colour);
                if label == "Gerätename" {
                    let fy = y + px(20.0);
                    c.rect(
                        x as u32 + px(150.0) as u32,
                        fy as u32,
                        px(160.0) as u32,
                        px(24.0) as u32,
                        panel,
                    );
                    c.lit(
                        x + px(158.0),
                        fy + px(4.0),
                        px(12.0),
                        &model(r, &brand),
                        ink,
                    );
                    y += px(26.0);
                }
                y += px(46.0 + r.below(14) as f32);
            }
        } else {
            // Devices: panels a few shades off the window, each a device's
            // name over its state; one phone named after its owner.
            c.lit(x, y, px(18.0), "Ausgabegeräte", ink);
            y += px(36.0);
            let named = r.below(4);
            let mut k = 0;
            while y < full as f32 {
                let ph = px(58.0) as u32;
                c.rect(
                    x0 + px(8.0) as u32,
                    y as u32,
                    ww - px(16.0) as u32,
                    ph,
                    panel,
                );
                let ty = y + px(10.0);
                if k == named {
                    let end = c.lit(x, ty, px(14.0), "Smartphone von ", ink);
                    c.pii("PERSON", end, ty, px(14.0), &owner.first, ink);
                } else {
                    let other = r.pick(BRANDS);
                    c.lit(x, ty, px(14.0), &model(r, other), ink);
                }
                c.lit(x, ty + px(20.0), px(11.0), r.pick(STATES), muted);
                y += ph as f32 + px(6.0);
                k += 1;
            }
        }
    }
    // Cut: the bottom through whatever stands there, at times the top.
    let bottom = full - r.below(u64::from(full / 5)) as u32;
    let top = if r.chance(30) {
        r.below(u64::from(px(20.0) as u32)) as u32
    } else {
        0
    };
    let img = image::imageops::crop_imm(&c.img, 0, top, w, bottom - top).to_image();
    let truth = c
        .truth
        .into_iter()
        .filter_map(|t| {
            let Location::Region {
                part,
                frame,
                x,
                y,
                width,
                height,
            } = t.location
            else {
                return Some(t);
            };
            let (y0, y1) = (y.max(top), (y + height).min(bottom));
            // Two rows or fewer left: not readable, and not a value.
            (y1 >= y0 + 3).then(|| Truth {
                entity: t.entity,
                location: Location::Region {
                    part,
                    frame,
                    x,
                    y: y0 - top,
                    width,
                    height: y1 - y0,
                },
            })
        })
        .collect();
    (png(&img), truth)
}

/// Two settings windows side by side in a dark theme (ADR-0072): a device
/// manager's rows of printed small print on the left, and on the right an
/// account with name and e-mail over panels a few shades off the window's
/// background, one reaching the bottom edge. The bottom edge cuts the left
/// window's last line, a paired phone named after its owner.
pub(super) fn settings_png(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let (user, owner) = (Person::de(r), Person::de(r));
    let (w, h) = (1200, 400);
    let mut c = Canvas::new(w, h, [44, 48, 56]);
    let (white, grey) = ([235, 235, 238], [196, 198, 204]);
    c.lit(20.0, 10.0, 13.0, "Geräte-Center", grey);
    c.lit(20.0, 40.0, 26.0, "Geräteeinstellungen", white);
    for (i, (label, lines)) in [
        (
            "Ruhezustand",
            [
                "Wählen Sie, wann sich das Gerät",
                "nach einer Pause abschaltet.",
            ],
        ),
        (
            "Signaltöne",
            [
                "Ist es eingeschaltet, hören Sie Töne",
                "bei jeder Aktion des Geräts.",
            ],
        ),
        (
            "Statuslicht",
            [
                "Ist es eingeschaltet, leuchtet das",
                "Licht während eines Anrufs.",
            ],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 100.0 + i as f32 * 80.0;
        c.lit(20.0, y, 15.0, label, white);
        c.lit(200.0, y, 13.0, lines[0], grey);
        c.lit(200.0, y + 17.0, 13.0, lines[1], grey);
    }
    c.lit(20.0, 345.0, 15.0, "Gekoppelte Geräte", white);
    // Its lower half is past the edge: the truth is what is left of it.
    let end = c.lit(200.0, h as f32 - 9.0, 15.0, "Smartphone von ", white);
    c.pii("PERSON", end, h as f32 - 9.0, 15.0, &owner.first, white);
    if let Some(Truth {
        location: Location::Region { y, height, .. },
        ..
    }) = c.truth.last_mut()
    {
        *height = (*height).min(h - *y);
    }
    c.rect(620, 0, w - 620, h, [32, 32, 36]);
    c.pii("PERSON", 690.0, 26.0, 15.0, &user.name(), white);
    c.pii("EMAIL_ADDRESS", 690.0, 48.0, 12.0, &user.email, white);
    for (i, item) in [
        "Startseite",
        "System",
        "Bluetooth und Geräte",
        "Netzwerk und Internet",
        "Datenschutz",
    ]
    .into_iter()
    .enumerate()
    {
        c.lit(640.0, 100.0 + i as f32 * 40.0, 14.0, item, white);
    }
    for (i, (title, under)) in [
        ("Lautsprecher", "Standardgerät"),
        ("Digitaler Ausgang", "Aktiviert"),
        ("Monitor", "Aktiviert"),
        ("Lautstärke", ""),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 90 + i as u32 * 72;
        c.rect(
            860,
            y,
            w - 860,
            if i == 3 { h - y } else { 66 },
            [43, 43, 48],
        );
        c.lit(880.0, y as f32 + 12.0, 14.0, title, white);
        c.lit(880.0, y as f32 + 34.0, 12.0, under, grey);
    }
    (png(&c.img), c.truth)
}

/// Creditors as a debt advice application lists them: firms, which are no
/// personal data where no party's label stands beside them (ADR-0059), and
/// some named like a person.
const CREDITORS: &[&str] = &[
    "Nordwerk Inkasso GmbH",
    "Elbtal Kredit AG",
    "Kessler Finance GmbH",
    "Rheinmark Bank AG",
    "Weststrom Energie GmbH",
    "Hansaruf Mobilfunk GmbH",
    "Seeblick Versand GmbH",
    "Lindgrün Versicherung AG",
    "Brückner Forderungsmanagement GmbH",
    "Ostkap Leasing GmbH",
];

/// The record view of a debt advice application (#161): a side bar whose
/// entries are the words that make a column's cells names (`Gläubiger`,
/// `Mandanten`, `Schuldner`), a header with two clients, their birth date
/// and phone, and a table of creditors, firms and two persons among them,
/// beside the side bar. Only the persons and their data are personal data.
pub(super) fn desk_png(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let (user, a, b) = (Person::de(r), Person::de(r), Person::de(r));
    let (lawyer, notary) = (Person::de(r), Person::de(r));
    let mut c = Canvas::new(1400, 760, [255, 255, 255]);
    c.rect(0, 0, 200, 760, [247, 248, 250]);
    // The application's name in the logo, not read as text: the side bar's
    // first entry heads the column of entries below it.
    c.rect(14, 12, 24, 24, [26, 84, 150]);
    c.pii(
        "PERSON",
        1080.0,
        14.0,
        13.0,
        &format!("{}, {}", user.last, user.first),
        INK,
    );
    let menu = [
        "Gläubiger",
        "Mandanten",
        "Wiedervorlagen",
        "Fristen",
        "Dokumente",
        "Zahlungen",
        "Berichte",
    ];
    // The first entry stands in the row of the header's fields, each a
    // column's head: the entries below are read as that column's cells.
    for (i, item) in menu.iter().enumerate() {
        c.lit(16.0, 50.0 + 26.0 * i as f32, 13.0, item, INK);
    }
    for (x, label, value) in [
        (760.0, "STATUS", "Warten auf Unterlagen"),
        (960.0, "BEARBEITER", "KMW"),
    ] {
        c.lit(x, 52.0, 10.0, label, MUTED);
        c.lit(x, 70.0, 12.0, value, INK);
    }
    let y = 50.0 + 26.0 * menu.len() as f32 + 14.0;
    let end = c.lit(16.0, y, 12.0, "Akte ", MUTED);
    c.pii(
        "PERSON",
        end,
        y,
        12.0,
        &format!("{}, {}", a.last, a.first),
        INK,
    );
    let sub = [
        "Stammdaten",
        "Einnahmen",
        "Schuldner & Vergleiche",
        "Ablage",
    ];
    for (i, item) in sub.iter().enumerate() {
        c.lit(28.0, y + 26.0 * (i + 1) as f32, 13.0, item, INK);
    }
    // The header: two clients, the first with birth date and phone.
    let end = c.lit(230.0, 52.0, 10.0, "HERR ", MUTED);
    c.pii(
        "PERSON",
        end,
        50.0,
        13.0,
        &format!("{}, {}", a.last, a.first),
        INK,
    );
    let small = [72, 78, 92];
    let end = c.pii("DATE_OF_BIRTH", 230.0, 70.0, 11.0, &a.dob_de(r), small);
    let end = c.lit(end, 70.0, 11.0, " · ", small);
    c.pii("PHONE_NUMBER", end, 70.0, 11.0, &a.phone, small);
    let end = c.lit(480.0, 52.0, 10.0, "FRAU ", MUTED);
    c.pii(
        "PERSON",
        end,
        50.0,
        13.0,
        &format!("{}, {}", b.last, b.first),
        INK,
    );
    c.lit(230.0, 110.0, 20.0, "Vergleichsberechnung", INK);
    // The table of creditors, its rows beside the side bar's entries.
    for (t, x) in [
        ("Name", 240.0),
        ("Forderung", 700.0),
        ("Rate", 880.0),
        ("Laufzeit", 1000.0),
    ] {
        c.lit(x, 160.0, 12.0, t, MUTED);
    }
    c.lit(240.0, 186.0, 12.0, "Einzelforderungen", INK);
    let rows = CREDITORS.len() + 2;
    let at = (r.below(4) + 2) as usize;
    for i in 0..rows {
        let y = 212.0 + 26.0 * i as f32;
        if i == at {
            let end = c.lit(256.0, y, 12.0, "Rechtsanwältin ", INK);
            c.pii("PERSON", end, y, 12.0, &lawyer.name(), INK);
        } else if i == at + 3 {
            let end = c.pii(
                "PERSON",
                256.0,
                y,
                12.0,
                &format!("{} {}", notary.last, notary.first),
                INK,
            );
            c.lit(end, y, 12.0, " Notar", INK);
        } else {
            let k = i - usize::from(i > at) - usize::from(i > at + 3);
            c.lit(256.0, y, 12.0, CREDITORS[k % CREDITORS.len()], INK);
        }
        let amount = 400 + r.below(14_000);
        c.lit(
            700.0,
            y,
            12.0,
            &format!("{}.{:03},00 €", amount / 1000, amount % 1000),
            INK,
        );
        c.lit(
            880.0,
            y,
            12.0,
            &format!("{},{:02} €", 1 + r.below(30), r.below(100)),
            INK,
        );
        c.lit(1000.0, y, 12.0, "36", INK);
    }
    (png(&c.img), c.truth)
}

/// Headline tiles of a news page (#161): a name in the genitive at the end
/// of a headline's line with a compound after it on the next, nouns that
/// mean persons (`Fahrzeugbesitzer`, `Rentner`), a bookmark bar of product
/// names, and a source line above each headline. Only the persons are
/// personal data.
pub(super) fn headlines_png(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let (p, q, s) = (Person::de(r), Person::de(r), Person::de(r));
    let mut c = Canvas::new(1200, 300, [36, 36, 42]);
    let white = [248, 248, 248];
    let grey = [170, 170, 176];
    for (i, mark) in ["Kalender", "Notizen", "Wetterdienst", "Ablage", "Fahrplan"]
        .iter()
        .enumerate()
    {
        c.lit(300.0 + 120.0 * i as f32, 12.0, 12.0, mark, grey);
    }
    let size = 19.0;
    let gap = 26.0;
    c.lit(20.0, 60.0, 12.0, "Morgenblatt · 3 Std.", grey);
    let end = c.lit(20.0, 84.0, size, "Kommentar: ", white);
    let end = c.pii("PERSON", end, 84.0, size, &p.last, white);
    c.lit(end, 84.0, size, "s", white);
    c.lit(20.0, 84.0 + gap, size, "Sommer-Bilanz bleibt offen", white);
    c.lit(420.0, 60.0, 12.0, "Kreisanzeiger · 1 Std.", grey);
    c.lit(420.0, 84.0, size, "Neue Regeln: Was", white);
    c.lit(420.0, 84.0 + gap, size, "Fahrzeugbesitzer jetzt", white);
    c.lit(420.0, 84.0 + 2.0 * gap, size, "wissen müssen", white);
    c.lit(820.0, 60.0, 12.0, "Abendkurier · 5 Std.", grey);
    let end = c.lit(820.0, 84.0, size, "Wirt ", white);
    let end = c.pii("PERSON", end, 84.0, size, &q.name(), white);
    c.lit(end, 84.0, size, " über", white);
    c.lit(820.0, 84.0 + gap, size, "Rentner und Stammgäste", white);
    c.lit(20.0, 200.0, 12.0, "Lokales · 2 Std.", grey);
    let end = c.lit(20.0, 224.0, size, "Bürgermeisterin ", white);
    let end = c.pii("PERSON", end, 224.0, size, &s.last, white);
    c.lit(end, 224.0, size, " eröffnet den Wochenmarkt", white);
    (png(&c.img), c.truth)
}

/// A word processor with its formatting marks shown (#161): a tab whose
/// label the window cuts short (`Brandtmeier, Jon..`), a line whose spaces
/// are drawn as `·`, and a table whose holder's cell wraps over two lines
/// beside the amounts of its rows. Each holds a person.
pub(super) fn workspace_png(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let (p, q, s) = (Person::de(r), Person::de(r), Person::de(r));
    let mut c = Canvas::new(1100, 460, [255, 255, 255]);
    c.rect(0, 0, 1100, 34, [232, 234, 240]);
    c.lit(14.0, 9.0, 13.0, "Notiz.docx – Textverarbeitung", INK);
    // The tabs: the second's label cut by its width.
    c.rect(0, 34, 1100, 30, [244, 245, 248]);
    c.lit(14.0, 42.0, 13.0, "Übersicht", INK);
    let cut: String = p.first.chars().take(3).collect();
    let end = c.lit(140.0, 42.0, 13.0, "Akte ", INK);
    let end = c.pii(
        "PERSON",
        end,
        42.0,
        13.0,
        &format!("{}, {cut}", p.last),
        INK,
    );
    c.lit(end, 42.0, 13.0, "..", INK);
    c.lit(340.0, 42.0, 13.0, "Vorlagen", INK);
    // The page.
    let end = c.lit(60.0, 96.0, 15.0, "Betreff: Akte ", INK);
    c.pii("PERSON", end, 96.0, 15.0, &p.name(), INK);
    let end = c.lit(60.0, 132.0, 15.0, "Rückruf·an·Herrn·", INK);
    let end = c.pii(
        "PERSON",
        end,
        132.0,
        15.0,
        &format!("{}·{}", q.first, q.last),
        INK,
    );
    c.lit(end, 132.0, 15.0, "·heute¶", INK);
    for (t, x) in [
        ("Kontoinhaber", 60.0),
        ("Verwendung", 330.0),
        ("Betrag", 700.0),
    ] {
        c.lit(x, 190.0, 13.0, t, MUTED);
    }
    for (i, amount) in ["2.247,43", "1.661,55"].iter().enumerate() {
        let y = 222.0 + 56.0 * i as f32;
        if i == 0 {
            c.pii("PERSON", 60.0, y, 14.0, &s.first, INK);
            c.pii("PERSON", 60.0, y + 17.0, 14.0, &s.last, INK);
        } else {
            c.lit(60.0, y + 8.0, 14.0, "Hausverwaltung Nord", INK);
        }
        c.lit(330.0, y + 8.0, 14.0, "Miete November", INK);
        c.lit(700.0, y + 8.0, 14.0, amount, INK);
    }
    (png(&c.img), c.truth)
}

/// Blue ink of a ballpoint pen.
const PEN: [u8; 3] = [24, 46, 128];
/// How large a hand writes on a form's line.
const HAND_SIZE: f32 = 34.0;

/// A scanned paper form filled in by hand (#59): the labels printed, the
/// values written in a hand (Caveat) on the lines beside them, each a little
/// off the line as a hand writes it.
pub(super) fn handwritten_png(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let (p, x) = (Person::de(r), Lookalikes::new(r));
    let mut c = Canvas::new(900, 520, [250, 249, 245]);
    c.lit(40.0, 28.0, 22.0, "Rückmeldebogen", INK);
    c.lit(
        40.0,
        62.0,
        14.0,
        &format!("Bitte in Druckbuchstaben ausfüllen · Vorgang {}", x.invoice),
        MUTED,
    );
    let rows = [
        (
            "Name, Vorname",
            "PERSON",
            format!("{}, {}", p.last, p.first),
        ),
        ("Geburtsdatum", "DATE_OF_BIRTH", p.dob_de(r)),
        ("Anschrift", "ADDRESS", format!("{}, {}", p.street, p.city)),
        ("Telefon", "PHONE_NUMBER", p.phone.clone()),
        ("IBAN", "IBAN_CODE", p.iban_spaced()),
    ];
    for (i, (label, entity, value)) in rows.iter().enumerate() {
        let y = 110.0 + 70.0 * i as f32;
        c.lit(40.0, y + 14.0, 15.0, label, MUTED);
        c.rect(220, y as u32 + 44, 640, 1, [150, 150, 150]);
        c.hand(true);
        let off = r.below(7) as f32 - 3.0;
        c.pii(entity, 228.0 + off, y + off, HAND_SIZE, value, PEN);
        c.hand(false);
    }
    (png(&c.img), c.truth)
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

/// A case list in a browser (ADR-0041, ADR-0042): the client (a person or
/// a firm, ADR-0059), a surname
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
        // A firm as a party is filled as a person (ADR-0059).
        if r.chance(25) {
            c.pii("PERSON", cols[3].1, y, 12.0, "Nordwerk GmbH", INK);
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

/// A dunning letter in an application's preview (ADR-0051): a tab bar with
/// the debtor's record cut short to `Surname, ...`, a heading in large type
/// with the dunning court's number after `Aktenzeichen:` (no slash), the
/// debtor and the address, and an order number of the same shape without a
/// word for a file before it.
pub(super) fn dunning_png(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let debtor = Person::de(r);
    let mut c = Canvas::new(1000, 420, [255, 255, 255]);
    c.rect(0, 0, 1000, 40, [229, 232, 240]);
    let mut x = 12.0;
    for tab in ["Übersicht", "Posteingang", "Fristen"] {
        x = c.lit(x, 12.0, 13.0, tab, INK) + 36.0;
    }
    // The debtor's tab, as narrow as the surname, its comma and the dots;
    // the surname is the personal data.
    let name = format!("{}, {}", debtor.last, debtor.first);
    let w = c.width(13.0, &format!("{}, ...", debtor.last));
    let shown = c.fit(13.0, &name, w);
    let rest = &shown[debtor.last.len()..];
    let end = c.pii("PERSON", x, 12.0, 13.0, &debtor.last, INK);
    c.lit(end, 12.0, 13.0, rest, INK);

    let number = format!("2{}{}", r.below(10), r.digits(9));
    c.lit(40.0, 70.0, 19.0, "Forderung der Nordwerk Finanz GmbH", INK);
    let court = r.pick(super::COURTS);
    c.lit(
        40.0,
        100.0,
        19.0,
        &format!("Vollstreckungsbescheid, {court} – Mahnabteilung –"),
        INK,
    );
    let end = c.lit(40.0, 130.0, 19.0, "Aktenzeichen: ", INK);
    c.pii("COURT_FILE_NUMBER", end, 130.0, 19.0, &number, INK);
    let end = c.lit(40.0, 160.0, 19.0, "zu Händen ", INK);
    c.pii("PERSON", end, 160.0, 19.0, &debtor.name(), INK);
    c.pii(
        "ADDRESS",
        40.0,
        190.0,
        19.0,
        &format!("{}, {}", debtor.street, debtor.city),
        INK,
    );
    c.lit(40.0, 250.0, 15.0, "Sehr geehrte Damen und Herren,", INK);
    c.lit(
        40.0,
        280.0,
        15.0,
        "in obiger Angelegenheit bestätigen wir den Eingang Ihres Schreibens.",
        INK,
    );
    let order = format!("2{}{}", r.below(10), r.digits(9));
    c.lit(
        40.0,
        310.0,
        15.0,
        &format!("Ihre Bestellung {order} ist davon nicht berührt."),
        INK,
    );
    (png(&c.img), c.truth)
}

/// An inbox of acknowledgements (ADR-0052): subjects with `Ihre Nachricht`
/// and `Ihr Anliegen`, which NER takes for names, and a row with a real
/// name beside them.
pub(super) fn inbox_png(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    const ROW: f32 = 28.0;
    let rows = 6;
    let mut c = Canvas::new(900, 48 + rows * ROW as u32, [255, 255, 255]);
    for (title, x) in [
        ("Erstellt", 10.0),
        ("Beschreibung", 150.0),
        ("Schlagworte", 600.0),
        ("E", 800.0),
    ] {
        c.lit(x, 12.0, 12.0, title, INK);
    }
    c.rect(0, 34, 900, 1, [210, 213, 220]);
    let named = r.below(rows.into()) as u32;
    for i in 0..rows {
        let y = 44.0 + i as f32 * ROW;
        let date = format!(
            "{:02}.11.2026 {:02}:{:02}",
            10 + r.below(8),
            8 + r.below(10),
            r.below(60)
        );
        c.lit(10.0, y, 12.0, &date, INK);
        if i == named {
            let end = c.lit(150.0, y, 12.0, "Schreiben zu Händen ", INK);
            c.pii("PERSON", end, y, 12.0, &Person::de(r).name(), INK);
        } else {
            let subject = match r.below(3) {
                0 => format!("Wir haben Ihre Nachricht vom {date} erhalten"),
                1 => "Ihre Nachricht hat uns erreicht".to_owned(),
                _ => format!("Ihr Anliegen SCM{}", r.digits(7)),
            };
            c.lit(150.0, y, 12.0, &subject, INK);
        }
        c.lit(600.0, y, 12.0, "GL Eingangsbestätigung", INK);
        c.lit(800.0, y, 12.0, r.pick(&["*MS", "*FW"]), INK);
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

/// How a letter was scanned (ADR-0066): the resolution across and down,
/// whether it is grey or black and white, the JPEG quality it was stored at,
/// and how blurred the scanner's optics left it. Each is one a person still
/// reads.
#[derive(Clone, Copy)]
pub(super) struct Scan {
    pub dpi_x: u32,
    pub dpi_y: u32,
    pub black_and_white: bool,
    pub quality: u8,
    pub blur: f32,
    /// The size of the side column's print, in points.
    pub small_pt: f32,
}

/// An office scanner's default: 150 dpi, grey, compressed hard.
pub(super) const OFFICE: Scan = Scan {
    dpi_x: 150,
    dpi_y: 150,
    black_and_white: false,
    quality: 40,
    blur: 0.6,
    small_pt: 6.5,
};
/// A fax in standard mode: 200 dpi across, 100 down, black and white.
pub(super) const FAX: Scan = Scan {
    dpi_x: 200,
    dpi_y: 100,
    black_and_white: true,
    quality: 75,
    blur: 0.0,
    small_pt: 6.5,
};
/// A copier's scan to mail at its lowest setting: 120 dpi, grey.
pub(super) const COPIER: Scan = Scan {
    dpi_x: 120,
    dpi_y: 120,
    black_and_white: false,
    quality: 35,
    blur: 0.8,
    small_pt: 6.5,
};
/// A document scanner at 300 dpi, grey, a side column of 6 pt (ADR-0067):
/// finer than the 200 dpi veilq renders a page at.
pub(super) const FINE: Scan = Scan {
    dpi_x: 300,
    dpi_y: 300,
    black_and_white: false,
    quality: 60,
    blur: 0.5,
    small_pt: 6.0,
};
/// A multifunction device's scan at 100 dpi, black and white (ADR-0067):
/// coarse, but read by a person.
pub(super) const COARSE: Scan = Scan {
    dpi_x: 100,
    dpi_y: 100,
    black_and_white: true,
    quality: 60,
    blur: 0.0,
    small_pt: 6.5,
};

/// A letter on A4 as `scan` hands it on (ADR-0066): the recipient's name and
/// address, a side column in small print that names the policyholder, and a
/// sentence that names them again. Drawn at 200 dpi, where veilq reads a
/// PDF's page and the truth is, or at the scan's resolution where it is
/// finer (the truth then scaled to 200 dpi); scanned to the scan's
/// resolution and set into the PDF at it, so that PDFium renders it at
/// 200 dpi.
pub(super) fn scanned_letter_pdf(r: &mut Rng, scan: Scan) -> (Vec<u8>, Vec<Truth>) {
    let (p, q, x) = (Person::de(r), Person::de(r), Lookalikes::new(r));
    let dpi = scan.dpi_x.max(scan.dpi_y).max(200);
    // Positions are given at 200 dpi and drawn `k` times as far.
    let k = dpi as f32 / 200.0;
    let mut c = Canvas::new((1654.0 * k) as u32, (2339.0 * k) as u32, [250, 250, 248]);
    c.frame = Some(1);
    // A point is `dpi`/72 pixels.
    let pt = |v: f32| v * dpi as f32 / 72.0;
    c.lit(
        160.0 * k,
        410.0 * k,
        pt(6.0),
        "Beispiel Versicherung AG · Musterstraße 1 · 28195 Bremen",
        MUTED,
    );
    c.pii("PERSON", 175.0 * k, 460.0 * k, pt(10.0), &p.name(), INK);
    c.pii("ADDRESS", 175.0 * k, 505.0 * k, pt(10.0), &p.street, INK);
    c.pii("ADDRESS", 175.0 * k, 550.0 * k, pt(10.0), &p.city, INK);
    // The side column in small print, set flush right.
    let size = pt(scan.small_pt);
    let right = |c: &Canvas, s: &str| 1496.0 * k - c.width(size, s);
    let mut y = 300.0 * k;
    for line in [
        format!("Datum: {}", x.date),
        "Beispiel Versicherung AG".to_owned(),
        format!("Vertrag {}", x.invoice),
    ] {
        let at = right(&c, &line);
        c.lit(at, y, size, &line, INK);
        y += 30.0 * k;
    }
    let at = right(&c, &format!("Versicherungsnehmer: {}", q.name()));
    let end = c.lit(at, y, size, "Versicherungsnehmer: ", INK);
    c.pii("PERSON", end, y, size, &q.name(), INK);
    // No capitalised word starts a line of the body: NER read alone takes
    // `Ihre Anfrage` and `Ihnen …` for a person there, a reading of its own
    // this letter does not measure (ADR-0066, Consequences).
    c.lit(175.0 * k, 800.0 * k, pt(11.0), "zur Vertragsübersicht", INK);
    let size = pt(9.0);
    let end = c.lit(
        175.0 * k,
        880.0 * k,
        size,
        "zu dem Vertrag, den Sie für ",
        INK,
    );
    let end = c.pii("PERSON", end, 880.0 * k, size, &q.name(), INK);
    c.lit(
        end,
        880.0 * k,
        size,
        " abgeschlossen haben, senden wir",
        INK,
    );
    c.lit(
        175.0 * k,
        925.0 * k,
        size,
        &format!("die Übersicht zum {} über {} EUR mit.", x.date, x.amount),
        INK,
    );
    if dpi > 200 {
        for t in &mut c.truth {
            if let Location::Region {
                x,
                y,
                width,
                height,
                ..
            } = &mut t.location
            {
                let (x1, y1) = (*x + *width, *y + *height);
                *x = (*x as f32 / k) as u32;
                *y = (*y as f32 / k) as u32;
                *width = (x1 as f32 / k).ceil() as u32 - *x;
                *height = (y1 as f32 / k).ceil() as u32 - *y;
            }
        }
    }
    let (w, h) = c.img.dimensions();
    let img = if scan.blur > 0.0 {
        image::imageops::blur(&c.img, scan.blur)
    } else {
        c.img.clone()
    };
    let (sw, sh) = (w * scan.dpi_x / dpi, h * scan.dpi_y / dpi);
    let mut img = image::imageops::resize(&img, sw, sh, image::imageops::FilterType::Triangle);
    if scan.black_and_white {
        for px in img.pixels_mut() {
            let grey = (u32::from(px[0]) + u32::from(px[1]) + u32::from(px[2])) / 3;
            *px = Rgb(if grey < 170 {
                [0, 0, 0]
            } else {
                [255, 255, 255]
            });
        }
    }
    let mut data = Vec::new();
    JpegEncoder::new_with_quality(&mut data, scan.quality)
        .encode_image(&img)
        .expect("a JPEG");
    let (pw, ph) = (w as f32 * 72.0 / dpi as f32, h as f32 * 72.0 / dpi as f32);
    (pdf_of_jpeg_on(&data, sw, sh, pw, ph), c.truth)
}

/// A one-page PDF that is the JPEG `data` (`w` x `h` pixels at 200 dpi).
fn pdf_of_jpeg(data: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (pw, ph) = (w as f32 * 72.0 / 200.0, h as f32 * 72.0 / 200.0);
    pdf_of_jpeg_on(data, w, h, pw, ph)
}

/// A one-page PDF of `pw` x `ph` points that is the JPEG `data` (`w` x `h`
/// pixels), stretched over the page.
fn pdf_of_jpeg_on(data: &[u8], w: u32, h: u32, pw: f32, ph: f32) -> Vec<u8> {
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
