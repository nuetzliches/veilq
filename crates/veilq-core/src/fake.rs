//! `fake` (ADR-0006): a plausible value of the same type and shape.
//!
//! The same value of one type gets the same stand-in within a run (or, with a
//! pseudonym key, within the key), derived as HMAC-SHA256(seed, type, value):
//! a reader still sees that two places hold the same value. Without a key the
//! seed is random per run, so a stand-in says nothing about the value across
//! runs. Stand-ins are synthetic: names from a fixed list, mail under
//! `example.org`, IP addresses from the documentation ranges, identifiers
//! with valid check digits so that the output still parses where it is
//! checked.
//!
//! A name is replaced word by word: a first name by a first name of the same
//! gender (as far as a list of common first names, or its ending, tells),
//! a last name by a last name, each the same wherever it stands, so that
//! `Frau Albers` and `Mira Albers` keep belonging together. A mail address
//! follows the name in its local part (`mira.albers` as the stand-in of
//! `Mira Albers`). A phone number keeps its layout and gets a real area
//! code of the same length and a subscriber number in the 555 block.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::detect::validate;
use crate::finding::EntityType;

const FEMALE: &[&str] = &[
    "Anna", "Clara", "Emma", "Greta", "Ida", "Karla", "Marie", "Olga", "Rosa", "Thea",
];
const MALE: &[&str] = &[
    "Ben", "David", "Felix", "Hannes", "Jakob", "Lukas", "Noah", "Paul", "Simon", "Valentin",
];
/// Common first names whose ending does not tell their gender.
const KNOWN_FEMALE: &[&str] = &[
    "agnes",
    "astrid",
    "beate",
    "birgit",
    "carmen",
    "doris",
    "elisabeth",
    "ellen",
    "gudrun",
    "helen",
    "ines",
    "ingrid",
    "iris",
    "karin",
    "kerstin",
    "lilly",
    "lily",
    "maren",
    "miriam",
    "nadine",
    "petra",
    "ruth",
    "sigrid",
    "sophie",
    "susan",
    "ulrike",
    "ute",
    "elif",
    "emily",
    "lena",
    "mira",
    "jule",
    "frauke",
    "heike",
    "silke",
    "anke",
    "elke",
    "maike",
    "wiebke",
    "sarah",
    "mary",
    "rachel",
    "jennifer",
    "karen",
    "margaret",
    "elizabeth",
    "ann",
    "jane",
    "lisa",
    "laura",
    "lea",
    "leonie",
    "luise",
    "mia",
    "nele",
    "ronja",
    "zoe",
];
const KNOWN_MALE: &[&str] = &[
    "andrea", "luca", "nikita", "sascha", "jonas", "tobias", "lukas", "noah", "elias", "mattis",
    "kai", "uwe", "ole", "jan", "tom", "tim", "max", "ben", "paul", "felix", "jakob", "simon",
    "david", "michael", "thomas", "andreas", "stefan", "peter", "klaus", "hans", "jürgen", "frank",
    "uwe", "dirk", "jörg", "sven", "lars", "malte", "hauke", "mike", "joe", "john", "james",
    "robert", "william", "richard", "daniel", "mark", "steven", "kevin", "brian", "george",
    "edward", "mohammed", "ali", "mehmet", "leon", "finn", "luis", "henry", "theo", "anton",
    "emil", "oskar", "karl", "moritz", "philipp", "niklas", "julian", "fabian",
];
/// Real area codes by length; the subscriber numbers are made up (555).
const AREAS: &[&str] = &[
    "030", "040", "069", "089", "0421", "0221", "0211", "0711", "0351", "0511", "0231", "0201",
    "03641", "04101", "06221", "07071",
];
const AREAS_US: &[&str] = &["212", "415", "617", "312"];
const LAST: &[&str] = &[
    "Ahrens",
    "Bauer",
    "Claasen",
    "Dietz",
    "Eggers",
    "Fricke",
    "Gerdes",
    "Hansen",
    "Imhoff",
    "Jansen",
    "Kruse",
    "Lorenz",
    "Möller",
    "Nolte",
    "Ostermann",
    "Peters",
    "Quast",
    "Riemer",
    "Sander",
    "Thiel",
];
const STREETS: &[&str] = &[
    "Musterweg",
    "Beispielstraße",
    "Am Probehof",
    "Testallee",
    "Dummyring",
];
const PLACES: &[(&str, &str)] = &[
    ("10115", "Berlin"),
    ("20095", "Hamburg"),
    ("50667", "Köln"),
    ("80331", "München"),
    ("28195", "Bremen"),
];

#[derive(Clone)]
pub struct Faker {
    seed: Vec<u8>,
}

/// A stream of pseudo-random numbers for one value.
struct Draw {
    bytes: Vec<u8>,
    at: usize,
}

impl Draw {
    fn next(&mut self) -> u32 {
        let b = self.bytes[self.at % self.bytes.len()];
        self.at += 1;
        u32::from(b) << 8 | u32::from(self.bytes[(self.at * 7) % self.bytes.len()])
    }

    fn below(&mut self, n: usize) -> usize {
        self.next() as usize % n
    }

    fn digit(&mut self) -> char {
        char::from(b'0' + self.below(10) as u8)
    }

    fn pick<'a>(&mut self, list: &[&'a str]) -> &'a str {
        list[self.below(list.len())]
    }
}

impl Faker {
    pub fn new(seed: impl Into<Vec<u8>>) -> Self {
        Self { seed: seed.into() }
    }

    fn draw(&self, entity: &EntityType, value: &str) -> Draw {
        let mut bytes = Vec::new();
        for round in 0u8..4 {
            let mut mac =
                Hmac::<Sha256>::new_from_slice(&self.seed).expect("HMAC takes any key length");
            mac.update(&[round]);
            mac.update(entity.as_str().as_bytes());
            mac.update(&[0]);
            mac.update(value.as_bytes());
            bytes.extend_from_slice(&mac.finalize().into_bytes());
        }
        Draw { bytes, at: 0 }
    }

    /// The stand-in of a first name: one of the same gender, the same
    /// wherever the name stands.
    fn first_name(&self, word: &str) -> &'static str {
        let key = word.to_lowercase();
        let list = match gender(&key) {
            Some(true) => FEMALE,
            Some(false) => MALE,
            None if self.draw_kind("gender", &key).below(2) == 0 => FEMALE,
            None => MALE,
        };
        self.draw_kind("first", &key).pick(list)
    }

    /// The stand-in of a last name.
    fn last_name(&self, word: &str) -> &'static str {
        self.draw_kind("last", &word.to_lowercase()).pick(LAST)
    }

    fn draw_kind(&self, kind: &str, word: &str) -> Draw {
        self.draw(
            &EntityType::new("PERSON").expect("a valid type"),
            &format!("{kind}\0{word}"),
        )
    }

    /// A name word by word: the first word a first name (when there are more,
    /// or when it is a known first name), the last a last name, particles
    /// (`von der`) kept.
    fn person(&self, value: &str) -> String {
        let words: Vec<&str> = value.split_whitespace().collect();
        let n = words.len();
        words
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let letters = w.trim_matches(|c: char| !c.is_alphabetic());
                let (head, tail) = w.split_at(w.find(letters).unwrap_or(0));
                let tail = &tail[letters.len()..];
                let new = if letters.chars().next().is_some_and(char::is_lowercase) {
                    letters.to_owned()
                } else if i == 0 && (n > 1 || gender(&letters.to_lowercase()).is_some()) {
                    self.first_name(letters).to_owned()
                } else {
                    letters
                        .split('-')
                        .map(|part| self.last_name(part))
                        .collect::<Vec<_>>()
                        .join("-")
                };
                format!("{head}{new}{tail}")
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A mail address under `example.org` whose local part follows the name
    /// in it: `mira.albers` -> the stand-ins of `Mira` and `Albers`.
    fn email(&self, value: &str, d: &mut Draw) -> String {
        let local = value.split('@').next().unwrap_or("");
        let parts: Vec<&str> = local
            .split(|c: char| !c.is_alphabetic())
            .filter(|p| !p.is_empty())
            .collect();
        let ascii = |s: &str| {
            s.to_lowercase()
                .replace('ä', "ae")
                .replace('ö', "oe")
                .replace('ü', "ue")
                .replace('ß', "ss")
        };
        let mut new = String::new();
        let mut word = 0;
        let mut chars = local.chars().peekable();
        while let Some(c) = chars.peek().copied() {
            if c.is_alphabetic() {
                let part = parts[word];
                for _ in 0..part.chars().count() {
                    chars.next();
                }
                let stand_in = if part.chars().count() == 1 {
                    char::from(b'a' + d.below(26) as u8).to_string()
                } else if word == 0 && parts.len() > 1 {
                    self.first_name(part).to_owned()
                } else {
                    self.last_name(part).to_owned()
                };
                new.push_str(&ascii(&stand_in));
                word += 1;
            } else {
                new.push(if c.is_ascii_digit() { d.digit() } else { c });
                chars.next();
            }
        }
        if new.is_empty() {
            new = ascii(d.pick(LAST));
        }
        format!("{new}@example.org")
    }

    /// The stand-in for `value`, found as `entity`.
    pub fn fake(&self, entity: &EntityType, value: &str) -> String {
        let mut d = self.draw(entity, value);
        match entity.as_str() {
            "PERSON" => self.person(value),
            "EMAIL_ADDRESS" => self.email(value, &mut d),
            "PHONE_NUMBER" => fake_phone(&mut d, value),
            "ADDRESS" => {
                let (plz, place) = PLACES[d.below(PLACES.len())];
                let street = format!("{} {}", d.pick(STREETS), 1 + d.below(99));
                if value.chars().filter(char::is_ascii_digit).count() >= 5 {
                    format!("{street}, {plz} {place}")
                } else {
                    street
                }
            }
            "IP_ADDRESS" => format!("192.0.2.{}", 1 + d.below(254)),
            "IBAN_CODE" => fake_iban(&mut d, value),
            "TAX_ID" => {
                let mut digits: Vec<u32> = vec![1 + d.below(9) as u32];
                digits.extend((0..9).map(|_| d.below(10) as u32));
                digits.push(validate::tax_id_check_digit(&digits));
                reshape(
                    value,
                    &digits
                        .iter()
                        .map(|x| char::from_digit(*x, 10).unwrap())
                        .collect::<String>(),
                )
            }
            "CREDIT_CARD" => {
                let n = value.chars().filter(char::is_ascii_digit).count().max(13);
                let mut digits: Vec<u32> = vec![4];
                digits.extend((1..n - 1).map(|_| d.below(10) as u32));
                digits.push(luhn_digit(&digits));
                reshape(
                    value,
                    &digits
                        .iter()
                        .map(|x| char::from_digit(*x, 10).unwrap())
                        .collect::<String>(),
                )
            }
            "DATE_OF_BIRTH" => fake_date(&mut d, value),
            _ => shape(&mut d, value),
        }
    }
}

/// Whether a first name is female (`Some(true)`), male, or unknown: from a
/// list of common names, else from a German or English ending.
fn gender(lower: &str) -> Option<bool> {
    let known = |list: &[&str]| list.iter().any(|n| n.eq_ignore_ascii_case(lower));
    if known(KNOWN_MALE) || MALE.iter().any(|n| n.to_lowercase() == lower) {
        return Some(false);
    }
    if known(KNOWN_FEMALE) || FEMALE.iter().any(|n| n.to_lowercase() == lower) {
        return Some(true);
    }
    if lower.chars().count() < 3 {
        return None;
    }
    if lower.ends_with('a') || lower.ends_with("ine") || lower.ends_with("ette") {
        Some(true)
    } else {
        None
    }
}

/// A phone number in `value`'s layout: the country code kept, a real area
/// code of the same length, a subscriber number that starts with 555.
fn fake_phone(d: &mut Draw, value: &str) -> String {
    // The runs of digits, and which of them is the area code.
    let runs: Vec<&str> = value
        .split(|c: char| !c.is_ascii_digit())
        .filter(|r| !r.is_empty())
        .collect();
    let us = value.trim_start().starts_with('(') || value.trim_start().starts_with("+1");
    let mut i = 0;
    let mut new: Vec<String> = Vec::new();
    // A country code (`+49`, `0049`) and a trunk `(0)` stay.
    if value.trim_start().starts_with('+') || runs.first().is_some_and(|r| r.starts_with("00")) {
        new.push(runs[0].to_owned());
        i = 1;
    }
    while runs.get(i) == Some(&"0") {
        new.push("0".into());
        i += 1;
    }
    let rest: String = runs[i.min(runs.len())..].concat();
    // Area code: the first run when the number has more, else a code's
    // length of the one run.
    let area_len = if runs.len() > i + 1 {
        runs[i].len()
    } else {
        4.min(rest.len())
    };
    let candidates: Vec<String> = if us {
        AREAS_US.iter().map(|a| a.to_string()).collect()
    } else if rest.starts_with('0') {
        AREAS.iter().map(|a| a.to_string()).collect()
    } else {
        AREAS.iter().map(|a| a[1..].to_owned()).collect()
    };
    let fitting: Vec<&String> = candidates.iter().filter(|a| a.len() == area_len).collect();
    let area = if fitting.is_empty() {
        (0..area_len).map(|_| d.digit()).collect()
    } else {
        fitting[d.below(fitting.len())].clone()
    };
    let subscriber_len = rest.len().saturating_sub(area_len);
    let mut subscriber: String = "555".chars().take(subscriber_len).collect();
    if us && subscriber_len >= 7 {
        subscriber.push_str("01");
    }
    while subscriber.len() < subscriber_len {
        subscriber.push(d.digit());
    }
    let digits = format!("{}{area}{subscriber}", new.concat());
    reshape(value, &digits)
}

/// Every letter and digit of `value` replaced by a random one of its kind;
/// separators stay.
fn shape(d: &mut Draw, value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_digit() {
                d.digit()
            } else if c.is_uppercase() {
                char::from(b'A' + d.below(26) as u8)
            } else if c.is_lowercase() {
                char::from(b'a' + d.below(26) as u8)
            } else {
                c
            }
        })
        .collect()
}

/// `new`'s characters in `value`'s layout: its separators where they were.
fn reshape(value: &str, new: &str) -> String {
    let mut chars = new.chars();
    let mut out: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                chars.next().unwrap_or('0')
            } else {
                c
            }
        })
        .collect();
    out.extend(chars);
    out
}

fn fake_iban(d: &mut Draw, value: &str) -> String {
    let compact: String = value.chars().filter(char::is_ascii_alphanumeric).collect();
    let country = compact.get(..2).unwrap_or("DE").to_uppercase();
    let bban: String = (0..compact.len().saturating_sub(4).max(18))
        .map(|_| d.digit())
        .collect();
    // Check digits: 98 - (BBAN + country + "00") mod 97.
    let mut rem = 0u32;
    for c in bban.chars().chain(country.chars()).chain("00".chars()) {
        let v = c.to_digit(36).unwrap_or(0);
        rem = if v < 10 {
            (rem * 10 + v) % 97
        } else {
            (rem * 100 + v) % 97
        };
    }
    reshape(value, &format!("{country}{:02}{bban}", 98 - rem))
}

fn luhn_digit(digits: &[u32]) -> u32 {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &x)| {
            if i % 2 == 0 {
                let y = x * 2;
                if y > 9 { y - 9 } else { y }
            } else {
                x
            }
        })
        .sum();
    (10 - sum % 10) % 10
}

/// A date between 1950 and 2005 in the value's own layout.
fn fake_date(d: &mut Draw, value: &str) -> String {
    let (day, month, year) = (1 + d.below(28), 1 + d.below(12), 1950 + d.below(56));
    let groups: Vec<&str> = value
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .collect();
    match groups.as_slice() {
        [y, _, _] if y.len() == 4 => format!("{year}-{month:02}-{day:02}"),
        [_, _, y] => {
            let y = if y.len() == 2 {
                format!("{:02}", year % 100)
            } else {
                year.to_string()
            };
            let sep = if value.contains('/') { "/" } else { "." };
            format!("{day:02}{sep}{month:02}{sep}{y}")
        }
        _ => format!("{day:02}.{month:02}.{year}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(entity: &str, value: &str) -> String {
        Faker::new(*b"test seed").fake(&EntityType::new(entity).unwrap(), value)
    }

    #[test]
    fn stand_ins_keep_type_and_shape() {
        assert_eq!(fake("PERSON", "Mira Albers"), fake("PERSON", "Mira Albers"));
        assert_ne!(fake("PERSON", "Mira Albers"), "Mira Albers");
        assert!(fake("EMAIL_ADDRESS", "x@y.de").ends_with("@example.org"));
        let iban = fake("IBAN_CODE", "DE89 3704 0044 0532 0130 00");
        assert!(validate::iban(&iban), "{iban}");
        assert_eq!(iban.len(), "DE89 3704 0044 0532 0130 00".len());
        let tax = fake("TAX_ID", "57 381 942 656");
        assert!(validate::tax_id_de(&tax), "{tax}");
        assert!(validate::luhn(&fake("CREDIT_CARD", "4111 1111 1111 1111")));
        assert!(validate::date(&fake("DATE_OF_BIRTH", "14.03.1987")));
        assert!(fake("IP_ADDRESS", "203.0.113.24").starts_with("192.0.2."));
        let plate = fake("LICENSE_PLATE", "HB-MA 412");
        assert_eq!(plate.len(), 9);
        assert_eq!(&plate[2..3], "-");
    }

    #[test]
    fn a_name_keeps_its_gender_and_its_parts() {
        let female = [
            "Anna", "Clara", "Emma", "Greta", "Ida", "Karla", "Marie", "Olga", "Rosa", "Thea",
        ];
        for (value, first_is_female) in [
            ("Mira Albers", true),
            ("Tobias Kern", false),
            ("Lena Vogt", true),
        ] {
            let new = fake("PERSON", value);
            let first = new.split(' ').next().unwrap();
            assert_eq!(female.contains(&first), first_is_female, "{value} -> {new}");
        }
        // A last name alone gets the same stand-in as in the full name.
        let full = fake("PERSON", "Mira Albers");
        assert!(
            full.ends_with(&format!(" {}", fake("PERSON", "Albers"))),
            "{full}"
        );
    }

    #[test]
    fn a_mail_follows_the_name_it_holds() {
        let name = fake("PERSON", "Mira Albers")
            .to_lowercase()
            .replace(' ', ".");
        let name = name.replace('ö', "oe");
        assert_eq!(
            fake("EMAIL_ADDRESS", "mira.albers@example.com"),
            format!("{name}@example.org")
        );
    }

    #[test]
    fn a_phone_number_keeps_its_layout_with_a_real_area_code() {
        for value in [
            "0421 5550173",
            "+49 421 5550173",
            "+49 (0)421 555 0173",
            "030/5551234",
            "(212) 555-0147",
        ] {
            let new = fake("PHONE_NUMBER", value);
            let layout = |s: &str| {
                s.chars()
                    .map(|c| if c.is_ascii_digit() { '9' } else { c })
                    .collect::<String>()
            };
            assert_eq!(layout(&new), layout(value), "{value} -> {new}");
            let digits: String = new.chars().filter(char::is_ascii_digit).collect();
            assert!(digits.contains("555"), "{value} -> {new}");
        }
        let new = fake("PHONE_NUMBER", "0421 5550173");
        let area = new.split(' ').next().unwrap();
        assert!(AREAS.contains(&area), "{new}");
    }

    #[test]
    fn another_seed_gives_another_stand_in() {
        let p = EntityType::new("PHONE_NUMBER").unwrap();
        let a = Faker::new(*b"one").fake(&p, "0421 5550173");
        let b = Faker::new(*b"two").fake(&p, "0421 5550173");
        assert_ne!(a, b);
    }
}
