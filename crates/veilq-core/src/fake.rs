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

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::detect::validate;
use crate::finding::EntityType;

const FIRST: &[&str] = &[
    "Anna", "Ben", "Clara", "David", "Emma", "Felix", "Greta", "Hannes", "Ida", "Jakob", "Karla",
    "Lukas", "Marie", "Noah", "Olga", "Paul", "Rosa", "Simon", "Thea", "Valentin",
];
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

    /// The stand-in for `value`, found as `entity`.
    pub fn fake(&self, entity: &EntityType, value: &str) -> String {
        let mut d = self.draw(entity, value);
        match entity.as_str() {
            "PERSON" => {
                let words = value.split_whitespace().count();
                let last = d.pick(LAST);
                if words >= 2 {
                    format!("{} {last}", d.pick(FIRST))
                } else {
                    last.to_owned()
                }
            }
            "EMAIL_ADDRESS" => format!(
                "{}.{}@example.org",
                d.pick(FIRST).to_lowercase(),
                d.pick(LAST).to_lowercase().replace('ö', "oe")
            ),
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
    fn another_seed_gives_another_stand_in() {
        let p = EntityType::new("PHONE_NUMBER").unwrap();
        let a = Faker::new(*b"one").fake(&p, "0421 5550173");
        let b = Faker::new(*b"two").fake(&p, "0421 5550173");
        assert_ne!(a, b);
    }
}
