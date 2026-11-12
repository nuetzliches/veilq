//! The generator of the synthetic corpus (ADR-0013): documents per locale and
//! format, each with a `.truth.json` that names every piece of personal data
//! in it as a finding would (type and location, no detector, no confidence).
//!
//! Every value is made up: names from short lists of common first and last
//! names, mail under `example.org/.com/.net`, IP addresses from the
//! documentation ranges, identifiers with valid check digits. Each document
//! also holds look-alikes that are not personal data (invoice numbers,
//! versions, dates without a birth context, court names): they are not in the
//! truth, so a finding there is a false positive.

use std::path::Path;

use serde::{Deserialize, Serialize};
use veilq_core::detect::validate;
use veilq_core::{EntityType, Location};

mod visual;

/// One piece of personal data a document holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Truth {
    pub entity: EntityType,
    pub location: Location,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TruthFile {
    pub version: u32,
    /// The generator's seed, so the document can be made again.
    pub seed: u64,
    pub findings: Vec<Truth>,
}

/// splitmix64: small, fast, the same on every platform.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a, T: ?Sized>(&mut self, list: &[&'a T]) -> &'a T {
        list[self.below(list.len() as u64) as usize]
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn digits(&mut self, n: usize) -> String {
        (0..n)
            .map(|_| char::from(b'0' + self.below(10) as u8))
            .collect()
    }
}

// ------------------------------------------------------------------ values

const FIRST_DE: &[&str] = &[
    "Lena", "Jonas", "Mira", "Tobias", "Ida", "Felix", "Hanna", "Lukas", "Sophie", "Paul",
    "Emilia", "Moritz", "Clara", "Jan", "Greta", "Niklas", "Frieda", "Malte", "Wiebke", "Hauke",
];
const LAST_DE: &[&str] = &[
    "Vogt",
    "Brandt",
    "Albers",
    "Kern",
    "Sommer",
    "Meyer",
    "Hoffmann",
    "Schröder",
    "Wagner",
    "Becker",
    "Krüger",
    "Lange",
    "Peters",
    "Haas",
    "Kühn",
    "Friedrich",
    "Lorenz",
    "Bergmann",
    "Engel",
    "Seidel",
];
const STREETS_DE: &[&str] = &[
    "Lindenweg",
    "Ahornstraße",
    "Bahnhofstraße",
    "Schillerplatz",
    "Rosengasse",
    "Birkenallee",
    "Mühlenring",
    "Deichstraße",
    "Kirchsteig",
    "Wiesenweg",
    "Goethestraße",
    "Hauptstr.",
];
const CITIES_DE: &[&str] = &[
    "28195 Bremen",
    "20095 Hamburg",
    "10115 Berlin",
    "50667 Köln",
    "80331 München",
    "01067 Dresden",
    "04109 Leipzig",
    "30159 Hannover",
    "60311 Frankfurt am Main",
];
const AREAS_DE: &[&str] = &[
    "0421", "040", "030", "0221", "089", "0351", "0341", "0511", "069",
];
const PLATE_CODES: &[&str] = &["HB", "HH", "B", "K", "M", "DD", "L", "H", "F", "OL", "OS"];
const COURTS: &[&str] = &[
    "Landgericht Bremen",
    "Amtsgericht Hamburg",
    "Landgericht Köln",
    "Amtsgericht München",
];
const MONTHS_DE: &[&str] = &[
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];
const FIRST_EN: &[&str] = &[
    "Emily", "James", "Olivia", "Noah", "Ava", "Liam", "Grace", "Henry", "Chloe", "Oscar",
];
const LAST_EN: &[&str] = &[
    "Carter", "Brooks", "Hayes", "Morgan", "Reed", "Foster", "Bennett", "Hughes", "Porter", "Wells",
];
const MONTHS_EN: &[&str] = &[
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DOMAINS: &[&str] = &["example.org", "example.com", "example.net"];

/// A made-up person and everything a document may say about them.
struct Person {
    first: String,
    last: String,
    email: String,
    phone: String,
    iban: String,
    day: u64,
    month: u64,
    year: u64,
    street: String,
    city: String,
    tax_id: String,
    plate: String,
    ip: String,
    ssn_us: String,
}

impl Person {
    fn de(r: &mut Rng) -> Self {
        let first = r.pick(FIRST_DE).to_owned();
        let last = r.pick(LAST_DE).to_owned();
        Self::with(r, first, last, true)
    }

    fn en(r: &mut Rng) -> Self {
        let first = r.pick(FIRST_EN).to_owned();
        let last = r.pick(LAST_EN).to_owned();
        Self::with(r, first, last, false)
    }

    fn with(r: &mut Rng, first: String, last: String, de: bool) -> Self {
        let ascii = |s: &str| {
            s.to_lowercase()
                .replace('ä', "ae")
                .replace('ö', "oe")
                .replace('ü', "ue")
                .replace('ß', "ss")
        };
        let email = match r.below(3) {
            0 => format!("{}.{}@{}", ascii(&first), ascii(&last), r.pick(DOMAINS)),
            1 => format!(
                "{}{}@{}",
                &ascii(&first)[..1],
                ascii(&last),
                r.pick(DOMAINS)
            ),
            _ => format!(
                "{}.{}{}@{}",
                ascii(&first),
                ascii(&last),
                r.below(90) + 10,
                r.pick(DOMAINS)
            ),
        };
        let phone = if de {
            match r.below(5) {
                0 => format!("{} 555{}", r.pick(AREAS_DE), r.digits(4)),
                1 => format!("+49 {} 555{}", &r.pick(AREAS_DE)[1..], r.digits(4)),
                2 => format!("+49 (0){} 555 {}", &r.pick(AREAS_DE)[1..], r.digits(4)),
                3 => format!("{}-555{}", r.pick(AREAS_DE), r.digits(4)),
                _ => format!("{}/555{}", r.pick(AREAS_DE), r.digits(4)),
            }
        } else {
            format!(
                "({}) 555-01{}",
                r.pick(&["212", "415", "617", "312"]),
                r.digits(2)
            )
        };
        let tax_id = {
            let mut d: Vec<u32> = vec![1 + r.below(9) as u32];
            d.extend((0..9).map(|_| r.below(10) as u32));
            d.push(validate::tax_id_check_digit(&d));
            d.iter().map(u32::to_string).collect::<String>()
        };
        let ssn_us = format!(
            "{}-{:02}-{:04}",
            100 + r.below(565),
            1 + r.below(98),
            1 + r.below(9998)
        );
        Self {
            email,
            phone,
            iban: iban(r),
            day: 1 + r.below(28),
            month: 1 + r.below(12),
            year: 1950 + r.below(56),
            street: format!(
                "{} {}{}",
                r.pick(STREETS_DE),
                1 + r.below(120),
                if r.chance(20) { "a" } else { "" }
            ),
            city: r.pick(CITIES_DE).to_owned(),
            tax_id,
            plate: format!(
                "{}-{}{} {}",
                r.pick(PLATE_CODES),
                char::from(b'A' + r.below(26) as u8),
                if r.chance(50) {
                    char::from(b'A' + r.below(26) as u8).to_string()
                } else {
                    String::new()
                },
                1 + r.below(999)
            ),
            ip: format!(
                "{}.{}",
                r.pick(&["203.0.113", "198.51.100", "192.0.2"]),
                1 + r.below(254)
            ),
            ssn_us,
            first,
            last,
        }
    }

    fn name(&self) -> String {
        format!("{} {}", self.first, self.last)
    }

    fn dob_de(&self, r: &mut Rng) -> String {
        match r.below(3) {
            0 => format!("{:02}.{:02}.{}", self.day, self.month, self.year),
            1 => format!("{}.{}.{}", self.day, self.month, self.year),
            _ => format!(
                "{}. {} {}",
                self.day,
                MONTHS_DE[self.month as usize - 1],
                self.year
            ),
        }
    }

    fn dob_iso(&self) -> String {
        format!("{}-{:02}-{:02}", self.year, self.month, self.day)
    }

    fn dob_en(&self, r: &mut Rng) -> String {
        if r.chance(50) {
            format!("{:02}/{:02}/{}", self.month, self.day, self.year)
        } else {
            format!(
                "{} {}, {}",
                MONTHS_EN[self.month as usize - 1],
                self.day,
                self.year
            )
        }
    }

    fn tax_id_spaced(&self) -> String {
        let t = &self.tax_id;
        format!("{} {} {} {}", &t[..2], &t[2..5], &t[5..8], &t[8..])
    }

    fn iban_spaced(&self) -> String {
        self.iban
            .as_bytes()
            .chunks(4)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A German IBAN with valid check digits.
fn iban(r: &mut Rng) -> String {
    iban_of(&r.digits(18))
}

/// A German IBAN of a bank code and an account number with leading zeros,
/// unspaced, as a payment form shows it: a run of zeros OCR may read one
/// short (ADR-0037).
fn iban_with_zeros(r: &mut Rng) -> String {
    iban_of(&format!("{}000000{}", r.digits(8), r.digits(4)))
}

/// The IBAN of the German `bban` (18 digits), with its check digits.
fn iban_of(bban: &str) -> String {
    let mut rem = 0u64;
    for c in format!("{bban}131400").chars() {
        rem = (rem * 10 + u64::from(c.to_digit(10).unwrap())) % 97;
    }
    format!("DE{:02}{bban}", 98 - rem)
}

/// Things that look like personal data and are not.
struct Lookalikes {
    invoice: String,
    version: String,
    date: String,
    amount: String,
    court_city: String,
    bad_iban: String,
}

impl Lookalikes {
    fn new(r: &mut Rng) -> Self {
        let mut bad = iban(r).into_bytes();
        let last = bad.len() - 1;
        bad[last] = b'0' + (bad[last] - b'0' + 1) % 10;
        Self {
            invoice: format!("2026-{}-{}", r.digits(4), r.digits(4)),
            version: format!(
                "{}.{}.{}.{}",
                1 + r.below(20),
                r.below(10),
                r.below(30),
                r.below(10)
            ),
            date: format!("{:02}.{:02}.2026", 1 + r.below(28), 1 + r.below(12)),
            amount: format!("{},{:02}", 10 + r.below(990), r.below(100)),
            court_city: r.pick(COURTS).to_owned(),
            bad_iban: String::from_utf8(bad).unwrap(),
        }
    }
}

// ------------------------------------------------------------------ builders

/// A text with its truth: literal pieces and pieces of personal data.
#[derive(Default)]
struct Text {
    s: String,
    truth: Vec<(String, std::ops::Range<usize>)>,
}

impl Text {
    fn lit(&mut self, s: &str) -> &mut Self {
        self.s.push_str(s);
        self
    }

    fn pii(&mut self, entity: &str, value: &str) -> &mut Self {
        let start = self.s.len();
        self.s.push_str(value);
        self.truth.push((entity.to_owned(), start..self.s.len()));
        self
    }

    /// Text truth: byte offsets in the file, line and column.
    fn finish(self) -> (String, Vec<Truth>) {
        let truth = self
            .truth
            .iter()
            .map(|(e, r)| {
                let before = &self.s[..r.start];
                let line = before.matches('\n').count() as u64 + 1;
                let column = before.rsplit('\n').next().unwrap_or("").chars().count() as u64 + 1;
                Truth {
                    entity: EntityType::new(e).unwrap(),
                    location: Location::Span {
                        part: None,
                        start: r.start as u64,
                        end: r.end as u64,
                        line,
                        column,
                    },
                }
            })
            .collect();
        (self.s, truth)
    }

    /// The truth of a value inside a JSON string or a CSV cell: offsets
    /// relative to the value.
    fn relative(&self) -> Vec<(String, std::ops::Range<usize>)> {
        self.truth.clone()
    }
}

/// A free-text note about `p` (for a CSV cell or a JSON value), German.
fn note_de(r: &mut Rng, p: &Person, other: &Person) -> Text {
    let mut t = Text::default();
    match r.below(5) {
        0 => {
            t.lit("Bruder ")
                .pii("PERSON", &other.name())
                .lit(" als Ansprechpartner");
        }
        // A name without a word that marks it: for NER (M3), until then a miss.
        4 => {
            t.pii("PERSON", &other.name())
                .lit(" ruft zurück unter ")
                .pii("PHONE_NUMBER", &other.phone);
        }
        1 => {
            t.lit("Rückruf erbeten unter ")
                .pii("PHONE_NUMBER", &p.phone);
        }
        2 => {
            t.lit("Kfz ")
                .pii("LICENSE_PLATE", &p.plate)
                .lit(", Kennzeichen geprüft");
        }
        _ => {
            t.lit("keine Besonderheiten");
        }
    }
    t
}

// ------------------------------------------------------------------ documents

/// A document of the corpus.
pub struct Doc {
    /// Relative to the corpus root: `de/text/letter-1.txt`.
    pub path: String,
    pub content: Vec<u8>,
    pub truth: Vec<Truth>,
}

fn letter_de(r: &mut Rng) -> (String, Vec<Truth>) {
    let p = Person::de(r);
    let q = Person::de(r);
    let x = Lookalikes::new(r);
    let mut t = Text::default();
    t.lit(&format!(
        "Betreff: Vorgang {} vom {}\n\n",
        x.invoice, x.date
    ));
    if r.chance(50) {
        t.lit("Sehr geehrte Frau ")
            .pii("PERSON", &p.last)
            .lit(",\n\n");
    } else {
        t.lit("Hallo ").pii("PERSON", &p.name()).lit(",\n\n");
    }
    t.lit(if r.chance(50) { "Herr " } else { "Frau " })
        .pii("PERSON", &q.name())
        .lit(" (geb. ")
        .pii("DATE_OF_BIRTH", &q.dob_de(r))
        .lit(") hat angerufen und bittet um Rückruf unter ")
        .pii("PHONE_NUMBER", &q.phone)
        .lit(" oder per Mail an ")
        .pii("EMAIL_ADDRESS", &q.email)
        .lit(".\n");
    if r.chance(70) {
        t.lit("Anschrift: ")
            .pii("ADDRESS", &format!("{}, {}", q.street, q.city))
            .lit(".\n");
    }
    if r.chance(70) {
        t.lit("Bitte überweisen Sie ")
            .lit(&x.amount)
            .lit(" EUR auf IBAN ")
            .pii("IBAN_CODE", &q.iban_spaced())
            .lit(", Kontoinhaber ")
            .pii("PERSON", &q.name())
            .lit(".\n");
    }
    if r.chance(60) {
        t.lit("Steuer-ID: ")
            .pii("TAX_ID", &q.tax_id_spaced())
            .lit(".\n");
    }
    if r.chance(50) {
        t.lit("Das Fahrzeug mit dem Kennzeichen ")
            .pii("LICENSE_PLATE", &q.plate)
            .lit(" ist zugelassen.\n");
    }
    if r.chance(50) {
        let az = format!(
            "{} {} {}/{}",
            1 + r.below(30),
            r.pick(&["O", "C", "S", "T"]),
            1 + r.below(999),
            20 + r.below(7)
        );
        t.lit("Aktenzeichen ")
            .pii("COURT_FILE_NUMBER", &az)
            .lit(&format!(" beim {}.\n", x.court_city));
    }
    if r.chance(50) {
        // Without a word that marks the name: for NER (M3).
        t.lit("Beim Termin war auch ")
            .pii("PERSON", &p.name())
            .lit(" anwesend.\n");
    }
    t.lit(&format!(
        "\nNicht personenbezogen: Rechnung {}, Version {}, Referenz {} (Prüfsumme falsch).\n",
        x.invoice, x.version, x.bad_iban
    ));
    t.lit("\nMit freundlichen Grüßen\n")
        .pii("PERSON", &p.name())
        .lit("\n");
    t.finish()
}

fn log_de(r: &mut Rng) -> (String, Vec<Truth>) {
    let x = Lookalikes::new(r);
    let mut t = Text::default();
    for i in 0..6 {
        let p = Person::de(r);
        let ts = format!("2026-11-02T08:{:02}:{:02}Z", 10 + i, r.below(60));
        t.lit(&ts);
        match r.below(6) {
            0 => {
                t.lit(" INFO  login ok user=")
                    .pii("EMAIL_ADDRESS", &p.email)
                    .lit(" ip=")
                    .pii("IP_ADDRESS", &p.ip);
            }
            5 => {
                t.lit(" INFO  session started user=")
                    .pii("PERSON", &p.name())
                    .lit(&format!(" client={}", x.version));
            }
            1 => {
                t.lit(" INFO  payment queued iban=")
                    .pii("IBAN_CODE", &p.iban)
                    .lit(&format!(" amount={}", x.amount.replace(',', ".")));
            }
            2 => {
                t.lit(" WARN  callback failed phone=")
                    .pii("PHONE_NUMBER", &p.phone);
            }
            3 => {
                t.lit(&format!(
                    " INFO  record {} opened by ",
                    r.below(9000) + 1000
                ))
                .pii("PERSON", &p.name());
            }
            _ => {
                t.lit(" DEBUG request body={\"name\":\"")
                    .pii("PERSON", &p.name())
                    .lit("\",\"birth\":\"")
                    .pii("DATE_OF_BIRTH", &p.dob_iso())
                    .lit("\"}");
            }
        }
        t.lit("\n");
    }
    t.lit(&format!(
        "2026-11-02T09:00:00Z INFO  healthcheck ok build={} order={}\n",
        x.version, x.invoice
    ));
    t.finish()
}

fn letter_en(r: &mut Rng) -> (String, Vec<Truth>) {
    let p = Person::en(r);
    let q = Person::en(r);
    let x = Lookalikes::new(r);
    let mut t = Text::default();
    t.lit(&format!("Subject: Case {}\n\n", x.invoice));
    t.lit("Dear Mr. ").pii("PERSON", &p.last).lit(",\n\n");
    t.lit("Ms. ")
        .pii("PERSON", &q.name())
        .lit(" (born ")
        .pii("DATE_OF_BIRTH", &q.dob_en(r))
        .lit(") asked for a call back on ")
        .pii("PHONE_NUMBER", &q.phone)
        .lit(" or at ")
        .pii("EMAIL_ADDRESS", &q.email)
        .lit(".\n");
    if r.chance(60) {
        t.lit("Her SSN is ")
            .pii("SOCIAL_SECURITY_NUMBER", &q.ssn_us)
            .lit(".\n");
    }
    t.lit(&format!(
        "Not personal: invoice {}, version {}.\n",
        x.invoice, x.version
    ));
    t.lit("\nBest regards,\n")
        .pii("PERSON", &p.name())
        .lit("\n");
    t.finish()
}

/// Pieces of personal data in a value: type and byte range.
type Spans = Vec<(String, std::ops::Range<usize>)>;

/// A JSON string value: its pointer and the truth inside it.
type Values = Vec<(String, String, Vec<(String, std::ops::Range<usize>)>)>;

fn json_truth(values: &Values, line: impl Fn(&str) -> u64) -> Vec<Truth> {
    values
        .iter()
        .flat_map(|(pointer, _, truth)| truth.iter().map(move |(e, r)| (pointer, e, r)))
        .map(|(pointer, e, r)| Truth {
            entity: EntityType::new(e).unwrap(),
            location: Location::Span {
                part: Some(pointer.clone()),
                start: r.start as u64,
                end: r.end as u64,
                line: line(pointer),
                column: 1,
            },
        })
        .collect()
}

/// One value with its truth: the whole value is `entity`.
fn whole(
    pointer: &str,
    entity: &str,
    value: &str,
) -> (String, String, Vec<(String, std::ops::Range<usize>)>) {
    (
        pointer.to_owned(),
        value.to_owned(),
        vec![(entity.to_owned(), 0..value.len())],
    )
}

fn customer_json(r: &mut Rng) -> (String, Vec<Truth>) {
    let p = Person::de(r);
    let q = Person::de(r);
    let note = note_de(r, &p, &q);
    let (plz, ort) = p.city.split_once(' ').unwrap();
    let values: Values = vec![
        whole("/name", "PERSON", &p.name()),
        whole("/geburtsdatum", "DATE_OF_BIRTH", &p.dob_iso()),
        whole("/anschrift/strasse", "ADDRESS", &p.street),
        whole("/kontakt/0/wert", "EMAIL_ADDRESS", &p.email),
        whole("/kontakt/1/wert", "PHONE_NUMBER", &p.phone),
        whole("/bank/iban", "IBAN_CODE", &p.iban_spaced()),
        whole("/steuer_id", "TAX_ID", &p.tax_id),
        ("/notiz".into(), note.s.clone(), note.relative()),
    ];
    let v = |ptr: &str| values.iter().find(|(p, _, _)| p == ptr).unwrap().1.clone();
    let doc = serde_json::json!({
        "id": r.below(9000) + 1000,
        "name": v("/name"),
        "geburtsdatum": v("/geburtsdatum"),
        "anschrift": { "strasse": v("/anschrift/strasse"), "plz": plz, "ort": ort },
        "kontakt": [ { "typ": "mail", "wert": v("/kontakt/0/wert") }, { "typ": "tel", "wert": v("/kontakt/1/wert") } ],
        "bank": { "iban": v("/bank/iban") },
        "steuer_id": v("/steuer_id"),
        "notiz": v("/notiz"),
        "erstellt": "2026-11-02",
        "version": Lookalikes::new(r).version,
    });
    let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
    // A JSON document's findings are matched by pointer; the line is 1.
    (text, json_truth(&values, |_| 1))
}

fn events_ndjson(r: &mut Rng) -> (String, Vec<Truth>) {
    let mut out = String::new();
    let mut truth = Vec::new();
    for line in 1..=6u64 {
        let p = Person::de(r);
        let q = Person::de(r);
        let (doc, values): (serde_json::Value, Values) = match r.below(4) {
            0 => {
                let values = vec![
                    whole("/user/email", "EMAIL_ADDRESS", &p.email),
                    whole("/user/ip", "IP_ADDRESS", &p.ip),
                ];
                (
                    serde_json::json!({ "event": "login", "user": { "email": p.email, "ip": p.ip } }),
                    values,
                )
            }
            1 => {
                let values = vec![whole("/iban", "IBAN_CODE", &p.iban)];
                (
                    serde_json::json!({ "event": "payment", "iban": p.iban, "amount": 120.5 }),
                    values,
                )
            }
            2 => {
                let values = vec![whole("/by", "PERSON", &p.name())];
                (
                    serde_json::json!({ "event": "open", "record": r.below(9000) + 1000, "by": p.name() }),
                    values,
                )
            }
            _ => {
                let note = note_de(r, &p, &q);
                let values = vec![("/text".into(), note.s.clone(), note.relative())];
                (
                    serde_json::json!({ "event": "note", "text": note.s }),
                    values,
                )
            }
        };
        out.push_str(&serde_json::to_string(&doc).unwrap());
        out.push('\n');
        truth.extend(json_truth(&values, |_| line));
    }
    (out, truth)
}

/// A CSV cell: its text and truth.
fn csv_quote(s: &str, d: char) -> String {
    if s.contains(d) || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

fn customers_csv(r: &mut Rng, de: bool) -> (String, Vec<Truth>) {
    let d = if de { ';' } else { ',' };
    let header: Vec<&str> = if de {
        vec![
            "id",
            "name",
            "email",
            "telefon",
            "iban",
            "geburtsdatum",
            "notiz",
        ]
    } else {
        vec!["id", "name", "email", "phone", "date_of_birth", "ssn"]
    };
    let mut out = header.join(&d.to_string()) + "\n";
    let mut truth = Vec::new();
    for row in 2..=6u64 {
        let p = if de { Person::de(r) } else { Person::en(r) };
        let q = Person::de(r);
        let mut cells: Vec<(String, Spans)> = vec![(format!("{}", row - 1), vec![])];
        let whole_cell = |entity: &str, value: String| {
            let len = value.len();
            (value, vec![(entity.to_owned(), 0..len)])
        };
        cells.push(whole_cell("PERSON", p.name()));
        cells.push(whole_cell("EMAIL_ADDRESS", p.email.clone()));
        cells.push(whole_cell("PHONE_NUMBER", p.phone.clone()));
        if de {
            cells.push(if r.chance(80) {
                whole_cell("IBAN_CODE", p.iban_spaced())
            } else {
                (String::new(), vec![])
            });
            cells.push(whole_cell(
                "DATE_OF_BIRTH",
                if r.chance(50) {
                    p.dob_de(r)
                } else {
                    p.dob_iso()
                },
            ));
            let note = note_de(r, &p, &q);
            cells.push((note.s.clone(), note.relative()));
        } else {
            cells.push(whole_cell("DATE_OF_BIRTH", p.dob_en(r)));
            cells.push(whole_cell("SOCIAL_SECURITY_NUMBER", p.ssn_us.clone()));
        }
        let line: Vec<String> = cells.iter().map(|(c, _)| csv_quote(c, d)).collect();
        out.push_str(&line.join(&d.to_string()));
        out.push('\n');
        for (column, (_, t)) in cells.iter().enumerate() {
            for (e, range) in t {
                truth.push(Truth {
                    entity: EntityType::new(e).unwrap(),
                    location: Location::Cell {
                        sheet: None,
                        row,
                        column: column as u64 + 1,
                        start: range.start as u64,
                        end: range.end as u64,
                    },
                });
            }
        }
    }
    (out, truth)
}

/// The documents of one seed.
pub fn corpus(seed: u64) -> Vec<Doc> {
    let mut r = Rng::new(seed);
    let mut docs = Vec::new();
    let mut add = |path: String, (content, truth): (String, Vec<Truth>)| {
        docs.push(Doc {
            path,
            content: content.into_bytes(),
            truth,
        })
    };
    for i in 1..=3 {
        add(format!("de/text/letter-{i}.txt"), letter_de(&mut r));
        add(format!("de/text/app-{i}.log"), log_de(&mut r));
        add(
            format!("de/csv/customers-{i}.csv"),
            customers_csv(&mut r, true),
        );
        add(
            format!("de/ndjson/events-{i}.ndjson"),
            events_ndjson(&mut r),
        );
        add(format!("de/json/customer-{i}.json"), customer_json(&mut r));
        add(format!("en/text/letter-{i}.txt"), letter_en(&mut r));
        add(
            format!("en/csv/customers-{i}.csv"),
            customers_csv(&mut r, false),
        );
    }
    // Containers from a stream of their own: the text documents above stay
    // the same when these change.
    let mut r = Rng::new(seed ^ 0x5EED_0FF1_CE00_0000);
    for i in 1..=2 {
        for (path, (content, truth)) in [
            (format!("de/docx/contract-{i}.docx"), contract_docx(&mut r)),
            (
                format!("de/xlsx/customers-{i}.xlsx"),
                customers_xlsx(&mut r),
            ),
            (format!("de/eml/callback-{i}.eml"), callback_eml(&mut r)),
        ] {
            docs.push(Doc {
                path,
                content,
                truth,
            });
        }
    }
    // Pictures from a stream of their own as well (ADR-0023).
    let mut r = Rng::new(seed ^ 0x0C12_FACE_0000_0000);
    for (path, (content, truth)) in [
        ("de/png/screenshot-1.png", visual::screenshot_png(&mut r)),
        ("de/jpeg/form-1.jpg", visual::form_jpeg(&mut r)),
        ("de/gif/recording-1.gif", visual::recording_gif(&mut r)),
        ("de/pdf/scan-1.pdf", visual::scan_pdf(&mut r)),
        ("de/png/transfer-1.png", visual::transfer_png(&mut r)),
    ] {
        docs.push(Doc {
            path: path.to_owned(),
            content,
            truth,
        });
    }
    docs
}

// ------------------------------------------------------------------ containers

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn zip_of(parts: &[(&str, String)]) -> Vec<u8> {
    use std::io::Write;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in parts {
        w.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

/// Truth inside a numbered segment of a part (a paragraph, a header line).
fn segment_truth(part: &str, n: u64, t: &Text) -> Vec<Truth> {
    t.truth
        .iter()
        .map(|(e, r)| Truth {
            entity: EntityType::new(e).unwrap(),
            location: Location::Span {
                part: Some(part.to_owned()),
                start: r.start as u64,
                end: r.end as u64,
                line: n,
                column: t.s[..r.start].chars().count() as u64 + 1,
            },
        })
        .collect()
}

/// A paragraph as runs: the text cut at a few places, so that values run
/// across runs as Word writes them after an edit.
fn runs(r: &mut Rng, text: &str) -> String {
    let mut cuts: Vec<usize> = (0..3)
        .map(|_| r.below(text.len().max(1) as u64) as usize)
        .filter(|c| text.is_char_boundary(*c))
        .collect();
    cuts.sort();
    cuts.dedup();
    let mut out = String::new();
    let mut at = 0;
    for c in cuts.into_iter().chain([text.len()]) {
        if c > at {
            out.push_str(&format!(
                "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                xml_escape(&text[at..c])
            ));
            at = c;
        }
    }
    format!("<w:p>{out}</w:p>")
}

fn contract_docx(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let p = Person::de(r);
    let q = Person::de(r);
    let mut paragraphs: Vec<Text> = Vec::new();
    let mut t = Text::default();
    t.lit("Vertrag ").lit(&Lookalikes::new(r).invoice);
    paragraphs.push(t);
    let mut t = Text::default();
    t.lit("Zwischen der Beispiel GmbH und Frau ")
        .pii("PERSON", &p.name())
        .lit(", geb. ")
        .pii("DATE_OF_BIRTH", &p.dob_de(r))
        .lit(", wohnhaft ")
        .pii("ADDRESS", &format!("{}, {}", p.street, p.city))
        .lit(", wird folgender Vertrag geschlossen.");
    paragraphs.push(t);
    let mut t = Text::default();
    t.lit("Zahlungen auf IBAN ")
        .pii("IBAN_CODE", &p.iban_spaced())
        .lit(".");
    paragraphs.push(t);
    let mut t = Text::default();
    t.lit("Ansprechpartner ")
        .pii("PERSON", &q.name())
        .lit(", Telefon ")
        .pii("PHONE_NUMBER", &q.phone)
        .lit(".");
    paragraphs.push(t);
    let mut body = String::new();
    let mut truth = Vec::new();
    for (i, para) in paragraphs.iter().enumerate() {
        body.push_str(&runs(r, &para.s));
        truth.extend(segment_truth("word/document.xml", i as u64 + 1, para));
    }
    let doc = format!(
        "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
    );
    // docProps/core.xml: title (1), creator (2), last modified by (3), created (4).
    let author = Person::de(r).name();
    let editor = Person::de(r).name();
    let core = format!(
        "<cp:coreProperties xmlns:cp=\"c\" xmlns:dc=\"d\" xmlns:dcterms=\"t\"><dc:title>Vertrag</dc:title><dc:creator>{author}</dc:creator><cp:lastModifiedBy>{editor}</cp:lastModifiedBy><dcterms:created>2026-11-02T08:00:00Z</dcterms:created></cp:coreProperties>"
    );
    for (n, name) in [(2, &author), (3, &editor)] {
        truth.push(Truth {
            entity: EntityType::new("PERSON").unwrap(),
            location: Location::Span {
                part: Some("docProps/core.xml".into()),
                start: 0,
                end: name.len() as u64,
                line: n,
                column: 1,
            },
        });
    }
    let bytes = zip_of(&[
        (
            "[Content_Types].xml",
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>"
                .into(),
        ),
        ("word/document.xml", doc),
        ("docProps/core.xml", core),
    ]);
    (bytes, truth)
}

fn customers_xlsx(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    let header = ["name", "email", "telefon", "geburtsdatum"];
    let mut strings: Vec<String> = header.iter().map(|h| (*h).to_owned()).collect();
    let mut rows = String::new();
    let cell = |col: usize, row: u64| format!("{}{row}", (b'A' + col as u8) as char);
    rows.push_str("<row r=\"1\">");
    for (c, _) in header.iter().enumerate() {
        rows.push_str(&format!("<c r=\"{}\" t=\"s\"><v>{c}</v></c>", cell(c, 1)));
    }
    rows.push_str("</row>");
    let mut truth = Vec::new();
    let whole =
        |sheet: &str, row: u64, col: usize, entity: &str, value: &str, truth: &mut Vec<Truth>| {
            truth.push(Truth {
                entity: EntityType::new(entity).unwrap(),
                location: Location::Cell {
                    sheet: Some(sheet.into()),
                    row,
                    column: col as u64 + 1,
                    start: 0,
                    end: value.len() as u64,
                },
            });
        };
    for row in 2..=5u64 {
        let p = Person::de(r);
        let values = [
            ("PERSON", p.name()),
            ("EMAIL_ADDRESS", p.email.clone()),
            ("PHONE_NUMBER", p.phone.clone()),
            ("DATE_OF_BIRTH", p.dob_de(r)),
        ];
        rows.push_str(&format!("<row r=\"{row}\">"));
        for (c, (entity, v)) in values.iter().enumerate() {
            let i = strings.len();
            strings.push(v.clone());
            rows.push_str(&format!("<c r=\"{}\" t=\"s\"><v>{i}</v></c>", cell(c, row)));
            whole("Kunden", row, c, entity, v, &mut truth);
        }
        rows.push_str("</row>");
    }
    // A hidden sheet with a note.
    let q = Person::de(r);
    let note = format!("Steuer-ID {} von Herrn {}", q.tax_id, q.name());
    let note_i = strings.len();
    strings.push(note.clone());
    let tax_at = note.find(&q.tax_id).unwrap();
    let name_at = note.rfind(&q.name()).unwrap();
    for (entity, at, len) in [
        ("TAX_ID", tax_at, q.tax_id.len()),
        ("PERSON", name_at, q.name().len()),
    ] {
        truth.push(Truth {
            entity: EntityType::new(entity).unwrap(),
            location: Location::Cell {
                sheet: Some("Intern".into()),
                row: 1,
                column: 1,
                start: at as u64,
                end: (at + len) as u64,
            },
        });
    }
    let sst: String = strings
        .iter()
        .map(|s| format!("<si><t>{}</t></si>", xml_escape(s)))
        .collect();
    let bytes = zip_of(&[
        ("[Content_Types].xml", "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>".into()),
        ("xl/workbook.xml", "<workbook xmlns:r=\"r\"><sheets><sheet name=\"Kunden\" sheetId=\"1\" r:id=\"rId1\"/><sheet name=\"Intern\" sheetId=\"2\" state=\"hidden\" r:id=\"rId2\"/></sheets></workbook>".into()),
        ("xl/_rels/workbook.xml.rels", "<Relationships><Relationship Id=\"rId1\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Target=\"worksheets/sheet2.xml\"/></Relationships>".into()),
        ("xl/sharedStrings.xml", format!("<sst>{sst}</sst>")),
        ("xl/worksheets/sheet1.xml", format!("<worksheet><sheetData>{rows}</sheetData></worksheet>")),
        ("xl/worksheets/sheet2.xml", format!("<worksheet><sheetData><row r=\"1\"><c r=\"A1\" t=\"s\"><v>{note_i}</v></c></row></sheetData></worksheet>")),
    ]);
    (bytes, truth)
}

fn callback_eml(r: &mut Rng) -> (Vec<u8>, Vec<Truth>) {
    use base64::Engine as _;
    let from = Person::de(r);
    let to = Person::de(r);
    let p = Person::de(r);
    let mut truth = Vec::new();
    // Headers: each one a numbered line `Name: value`.
    let mut h1 = Text::default();
    h1.lit("From: ")
        .pii("PERSON", &from.name())
        .lit(" <")
        .pii("EMAIL_ADDRESS", &from.email)
        .lit(">");
    let mut h2 = Text::default();
    h2.lit("To: ")
        .pii("PERSON", &to.name())
        .lit(" <")
        .pii("EMAIL_ADDRESS", &to.email)
        .lit(">");
    let mut h3 = Text::default();
    h3.lit("Subject: Rückruf Frau ").pii("PERSON", &p.name());
    for (n, h) in [(1, &h1), (2, &h2), (3, &h3)] {
        truth.extend(segment_truth("headers", n, h));
    }
    let subject_b64 = base64::engine::general_purpose::STANDARD.encode(&h3.s["Subject: ".len()..]);
    // The body: plain text with a mail's CRLF, quoted-printable; its truth
    // in the decoded text, by line.
    let mut body = Text::default();
    body.lit("Hallo,\r\n\r\nFrau ")
        .pii("PERSON", &p.name())
        .lit(" bittet um Rückruf unter ")
        .pii("PHONE_NUMBER", &p.phone)
        .lit(".\r\nIhre IBAN: ")
        .pii("IBAN_CODE", &p.iban_spaced())
        .lit("\r\n\r\nViele Grüße\r\n")
        .pii("PERSON", &from.name())
        .lit("\r\n");
    let (text, body_truth) = body.finish();
    for t in body_truth {
        if let Location::Span {
            start,
            end,
            line,
            column,
            ..
        } = t.location
        {
            truth.push(Truth {
                entity: t.entity,
                location: Location::Span {
                    part: Some("part 1 text/plain".into()),
                    start,
                    end,
                    line,
                    column,
                },
            });
        }
    }
    let qp: String = text
        .bytes()
        .map(|b| match b {
            b'\r' | b'\n' => (b as char).to_string(),
            b'=' => "=3D".to_owned(),
            32..=126 => (b as char).to_string(),
            _ => format!("={b:02X}"),
        })
        .collect();
    // A CSV attachment, base64.
    let mut csv = String::from("name,telefon\n");
    let q = Person::de(r);
    csv.push_str(&format!("{},{}\n", q.name(), q.phone));
    for (col, entity, len) in [
        (1, "PERSON", q.name().len()),
        (2, "PHONE_NUMBER", q.phone.len()),
    ] {
        truth.push(Truth {
            entity: EntityType::new(entity).unwrap(),
            location: Location::Cell {
                sheet: Some("part 2 kunden.csv".into()),
                row: 2,
                column: col,
                start: 0,
                end: len as u64,
            },
        });
    }
    let csv_b64 = base64::engine::general_purpose::STANDARD.encode(csv);
    let mail = format!(
        "From: {} <{}>\r\nTo: {} <{}>\r\nSubject: =?utf-8?B?{subject_b64}?=\r\nDate: Mon, 02 Nov 2026 09:12:00 +0100\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n{qp}\r\n--b1\r\nContent-Type: text/csv; name=\"kunden.csv\"\r\nContent-Disposition: attachment; filename=\"kunden.csv\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{csv_b64}\r\n--b1--\r\n",
        from.name(),
        from.email,
        to.name(),
        to.email
    );
    (mail.into_bytes(), truth)
}

/// Writes the corpus of `seed` under `dir`.
pub fn write(dir: &Path, seed: u64) -> anyhow::Result<usize> {
    let docs = corpus(seed);
    for d in &docs {
        let path = dir.join(&d.path);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, &d.content)?;
        let truth = TruthFile {
            version: 1,
            seed,
            findings: d.truth.clone(),
        };
        std::fs::write(
            truth_path(&path),
            serde_json::to_string_pretty(&truth)? + "\n",
        )?;
    }
    Ok(docs.len())
}

/// Writes the long screen recording of `seed` under `dir`
/// (`de/gif/long-recording-1.gif`): not part of the corpus, too slow for
/// every check, but what reading only what changed is measured on (#12).
pub fn write_recording(dir: &Path, seed: u64) -> anyhow::Result<()> {
    let mut r = Rng::new(seed ^ 0x0C12_FACE_1000_0000);
    let (content, findings) = visual::long_recording_gif(&mut r);
    let path = dir.join("de/gif/long-recording-1.gif");
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, content)?;
    let truth = TruthFile {
        version: 1,
        seed,
        findings,
    };
    std::fs::write(
        truth_path(&path),
        serde_json::to_string_pretty(&truth)? + "\n",
    )?;
    Ok(())
}

/// `letter-1.txt` -> `letter-1.txt.truth.json`.
pub fn truth_path(doc: &Path) -> std::path::PathBuf {
    let mut name = doc.file_name().unwrap().to_os_string();
    name.push(".truth.json");
    doc.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_truth_points_at_what_it_names() {
        for d in corpus(7) {
            let Ok(content) = String::from_utf8(d.content.clone()) else {
                continue;
            };
            for t in &d.truth {
                if let Location::Span {
                    part: None,
                    start,
                    end,
                    ..
                } = t.location
                {
                    let v = &content[start as usize..end as usize];
                    assert!(
                        !v.is_empty() && !v.contains('\n') || t.entity.as_str() == "ADDRESS",
                        "{} {v:?}",
                        d.path
                    );
                }
            }
            assert!(!d.truth.is_empty(), "{}", d.path);
        }
    }

    #[test]
    fn a_pictures_truth_is_ink_inside_it() {
        for d in corpus(7) {
            let Some(format) = [("png", 0), ("jpg", 1), ("gif", 2)]
                .iter()
                .find(|(e, _)| d.path.ends_with(e))
            else {
                continue;
            };
            let img = image::load_from_memory(&d.content).unwrap().to_rgb8();
            for t in &d.truth {
                let Location::Region {
                    x,
                    y,
                    width,
                    height,
                    ..
                } = t.location
                else {
                    panic!("{}: {t:?}", d.path);
                };
                assert!(
                    x + width <= img.width() && y + height <= img.height(),
                    "{}",
                    d.path
                );
                if t.entity.as_str() == "METADATA" || format.1 == 2 {
                    continue;
                }
                // Dark pixels in the box: the value was drawn there.
                let ink = (y..y + height)
                    .flat_map(|yy| (x..x + width).map(move |xx| (xx, yy)))
                    .filter(|&(xx, yy)| img.get_pixel(xx, yy).0.iter().all(|c| *c < 120))
                    .count();
                assert!(ink > 0, "{} {t:?}", d.path);
            }
        }
    }

    #[test]
    fn a_seed_gives_the_same_corpus() {
        let a: Vec<Vec<u8>> = corpus(3).into_iter().map(|d| d.content).collect();
        let b: Vec<Vec<u8>> = corpus(3).into_iter().map(|d| d.content).collect();
        assert_eq!(a, b);
        assert_ne!(
            a,
            corpus(4).into_iter().map(|d| d.content).collect::<Vec<_>>()
        );
    }

    #[test]
    fn generated_identifiers_pass_their_checks() {
        let mut r = Rng::new(1);
        for _ in 0..50 {
            let p = Person::de(&mut r);
            assert!(validate::iban(&p.iban), "{}", p.iban);
            assert!(validate::tax_id_de(&p.tax_id), "{}", p.tax_id);
            assert!(validate::kfz_de(&p.plate), "{}", p.plate);
            assert!(validate::ssn_us(&p.ssn_us), "{}", p.ssn_us);
        }
    }
}
