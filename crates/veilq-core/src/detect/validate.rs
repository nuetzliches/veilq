//! Checks a pattern's match must pass (ADR-0012): checksums and ranges that a
//! regex cannot express. A failed check lowers the confidence instead of
//! dropping the match, so that a mistyped identifier next to its label is
//! still redacted.

/// The names a pattern's `validate` may use.
pub const VALIDATORS: &[&str] = &[
    "iban",
    "luhn",
    "tax_id_de",
    "ssn_de",
    "id_card_de",
    "date",
    "ipv4",
    "kfz_de",
    "ssn_us",
    "email_host",
];

/// The checks that are checksums: one misread character fails them. The
/// others check a form (a date, a district code, a host) that a misread
/// character rarely gives a value without it (ADR-0037).
const CHECKSUMS: &[&str] = &["iban", "luhn", "tax_id_de", "ssn_de", "id_card_de"];

/// Whether the check `name` is a checksum.
pub fn is_checksum(name: &str) -> bool {
    CHECKSUMS.contains(&name)
}

/// Whether `value` passes the check `name`. An unknown name never passes
/// (policies are checked against [`VALIDATORS`] when they load).
pub fn check(name: &str, value: &str) -> bool {
    match name {
        "iban" => iban(value),
        "luhn" => luhn(value),
        "tax_id_de" => tax_id_de(value),
        "ssn_de" => ssn_de(value),
        "id_card_de" => id_card_de(value),
        "date" => date(value),
        "ipv4" => ipv4(value),
        "kfz_de" => kfz_de(value),
        "ssn_us" => ssn_us(value),
        "email_host" => email_host(value),
        _ => false,
    }
}

fn compact(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn digits(value: &str) -> Vec<u32> {
    value.chars().filter_map(|c| c.to_digit(10)).collect()
}

/// ISO 13616: the country's length is not checked, the mod-97 sum is.
pub fn iban(value: &str) -> bool {
    let s = compact(value);
    if s.len() < 15 || s.len() > 34 || !s.as_bytes()[..2].iter().all(u8::is_ascii_uppercase) {
        return false;
    }
    let (head, tail) = s.split_at(4);
    let mut rem = 0u32;
    for c in tail.chars().chain(head.chars()) {
        let v = match c.to_digit(36) {
            Some(v) => v,
            None => return false,
        };
        rem = if v < 10 {
            (rem * 10 + v) % 97
        } else {
            (rem * 100 + v) % 97
        };
    }
    rem == 1
}

pub fn luhn(value: &str) -> bool {
    let d = digits(value);
    if d.len() < 12 {
        return false;
    }
    let sum: u32 = d
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &x)| {
            if i % 2 == 1 {
                let y = x * 2;
                if y > 9 { y - 9 } else { y }
            } else {
                x
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Steuerliche Identifikationsnummer: 11 digits, the first not 0, the last
/// an ISO 7064 MOD 11,10 check digit.
pub fn tax_id_de(value: &str) -> bool {
    let d = digits(value);
    d.len() == 11 && d[0] != 0 && tax_id_check_digit(&d[..10]) == d[10]
}

/// The check digit of the first ten digits of a Steuer-ID.
pub fn tax_id_check_digit(d: &[u32]) -> u32 {
    let mut product = 10;
    for &x in d {
        let mut sum = (x + product) % 10;
        if sum == 0 {
            sum = 10;
        }
        product = (sum * 2) % 11;
    }
    (11 - product) % 10
}

/// Versicherungsnummer of the German pension insurance: 2 digits area, 6
/// digits birth date (DDMMYY), a letter, 2 digits, a check digit.
pub fn ssn_de(value: &str) -> bool {
    let s = compact(value);
    let b = s.as_bytes();
    if b.len() != 12 || !b[8].is_ascii_uppercase() {
        return false;
    }
    if !b
        .iter()
        .enumerate()
        .all(|(i, c)| i == 8 || c.is_ascii_digit())
    {
        return false;
    }
    let day: u32 = s[2..4].parse().unwrap_or(0);
    let month: u32 = s[4..6].parse().unwrap_or(0);
    // Day + 50 marks a second number of the same person.
    if !((1..=31).contains(&(day % 50)) && (1..=12).contains(&month)) {
        return false;
    }
    ssn_de_check_digit(&s[..11]) == u32::from(b[11] - b'0')
}

/// The check digit of the first eleven characters of a Versicherungsnummer.
pub fn ssn_de_check_digit(first11: &str) -> u32 {
    let b = first11.as_bytes();
    let letter = u32::from(b[8] - b'A' + 1);
    let mut d: Vec<u32> = b[..8].iter().map(|c| u32::from(c - b'0')).collect();
    d.push(letter / 10);
    d.push(letter % 10);
    d.extend(b[9..11].iter().map(|c| u32::from(c - b'0')));
    const W: [u32; 12] = [2, 1, 2, 5, 7, 1, 2, 1, 2, 1, 2, 1];
    let sum: u32 = d
        .iter()
        .zip(W)
        .map(|(x, w)| {
            let p = x * w;
            p / 10 + p % 10
        })
        .sum();
    sum % 10
}

/// Personalausweis (since 2010): nine characters, a tenth check digit with
/// weights 7, 3, 1. Without the check digit, the alphabet is all there is to
/// check.
pub fn id_card_de(value: &str) -> bool {
    const ALPHABET: &[u8] = b"CFGHJKLMNPRTVWXYZ0123456789";
    let s = compact(value);
    let b = s.as_bytes();
    if !(b.len() == 9 || b.len() == 10) || !b[..9].iter().all(|c| ALPHABET.contains(c)) {
        return false;
    }
    if b.len() == 9 {
        return true;
    }
    id_card_check_digit(&s[..9]) == u32::from(b[9]).wrapping_sub(u32::from(b'0'))
}

pub fn id_card_check_digit(first9: &str) -> u32 {
    first9
        .chars()
        .zip([7, 3, 1].iter().cycle())
        .map(|(c, w)| c.to_digit(36).unwrap_or(0) * w)
        .sum::<u32>()
        % 10
}

/// A calendar date that can be a birth date: `14.03.1987`, `14.3.87`,
/// `1987-03-14`, `03/14/1987`, `14. März 1987`, `March 14, 1987`.
pub fn date(value: &str) -> bool {
    let nums: Vec<u32> = value
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    let month_name = month_from_name(value);
    let (day, month, year) = match (nums.as_slice(), month_name) {
        (&[d, y], Some(m)) if d <= 31 => (d, m, y),
        (&[y, m, d], None) if y >= 1000 => (d, m, y),
        // 03/14/1987 is month first; 14/03/1987 day first.
        (&[a, b, y], None) if value.contains('/') && a <= 12 && b > 12 => (b, a, y),
        (&[d, m, y], None) => (d, m, y),
        _ => return false,
    };
    let year = match year {
        0..=99 => 1900 + year,
        _ => year,
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1900..=2100).contains(&year) && (1..=days).contains(&day)
}

fn month_from_name(value: &str) -> Option<u32> {
    const NAMES: [(&str, u32); 24] = [
        ("januar", 1),
        ("februar", 2),
        ("märz", 3),
        ("april", 4),
        ("mai", 5),
        ("juni", 6),
        ("juli", 7),
        ("august", 8),
        ("september", 9),
        ("oktober", 10),
        ("november", 11),
        ("dezember", 12),
        ("january", 1),
        ("february", 2),
        ("march", 3),
        ("may", 5),
        ("june", 6),
        ("july", 7),
        ("october", 10),
        ("december", 12),
        ("jan", 1),
        ("feb", 2),
        ("mar", 3),
        ("dec", 12),
    ];
    let lower = value.to_lowercase();
    NAMES
        .iter()
        .find(|(n, _)| lower.split(|c: char| !c.is_alphabetic()).any(|w| w == *n))
        .map(|(_, m)| *m)
}

pub fn ipv4(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u16>().is_ok_and(|n| n <= 255))
}

/// A German licence plate whose district code is known. The list is a
/// selection; an unknown code lowers the confidence, it does not drop the
/// match.
pub fn kfz_de(value: &str) -> bool {
    let code: String = value
        .chars()
        .take_while(|c| c.is_alphabetic())
        .collect::<String>()
        .to_uppercase();
    KFZ_DE
        .lines()
        .any(|l| l.split_whitespace().any(|c| c == code))
}

const KFZ_DE: &str = include_str!("../../locales/de/kfz.txt");

/// US Social Security Number: area not 000, 666 or 9xx; group not 00; serial
/// not 0000.
pub fn ssn_us(value: &str) -> bool {
    let d = digits(value);
    if d.len() != 9 {
        return false;
    }
    let area = d[0] * 100 + d[1] * 10 + d[2];
    let group = d[3] * 10 + d[4];
    let serial = d[5..].iter().fold(0, |a, x| a * 10 + x);
    area != 0 && area != 666 && area < 900 && group != 0 && serial != 0
}

/// What follows the `@` of an address can be a host: not a package's tag
/// (`lodash@latest`), version (`actions/checkout@v4`) or commit
/// (`rust-cache@d1527c8f`), nor a hash's name (`node@sha256:...`).
pub fn email_host(value: &str) -> bool {
    const TAGS: &[&str] = &[
        "latest",
        "next",
        "beta",
        "alpha",
        "canary",
        "rc",
        "stable",
        "dev",
        "edge",
        "nightly",
        "experimental",
        "insiders",
        "main",
        "master",
        "head",
        "md5",
    ];
    let Some((_, host)) = value.rsplit_once('@') else {
        return false;
    };
    let host = host.to_lowercase();
    let digest = host
        .strip_prefix("sha")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    let version = host
        .strip_prefix('v')
        .is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit()));
    let commit = host.len() >= 7
        && host.chars().all(|c| c.is_ascii_hexdigit())
        && host.chars().any(|c| c.is_ascii_digit());
    !digest && !version && !commit && !TAGS.contains(&host.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums() {
        assert!(iban("DE89 3704 0044 0532 0130 00"));
        assert!(iban("DE44500105175407324931"));
        assert!(!iban("DE89 3704 0044 0532 0130 01"));
        assert!(iban("GB82 WEST 1234 5698 7654 32"));
        assert!(email_host("mira.albers@exampl"));
        assert!(email_host("root@buildhost-3"));
        assert!(!email_host("lodash@latest"));
        assert!(!email_host("node@sha256"));
        assert!(!email_host("actions/checkout@v4"));
        assert!(!email_host(
            "rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0"
        ));
        assert!(email_host("mira@vogt-kanzlei"));
        assert!(email_host("mira@cafe"));
        assert!(luhn("4111 1111 1111 1111"));
        assert!(!luhn("4111 1111 1111 1112"));
        assert!(tax_id_de("57 381 942 656"));
        assert!(!tax_id_de("57 381 942 065"));
        assert!(id_card_de("L01X00T47"));
        assert!(id_card_de(&format!(
            "L01X00T47{}",
            id_card_check_digit("L01X00T47")
        )));
        assert!(!id_card_de("A01X00T47"));
        let ssn = format!("15070649C10{}", ssn_de_check_digit("15070649C10"));
        assert!(ssn_de(&ssn), "{ssn}");
        let wrong = format!(
            "15070649C10{}",
            (ssn_de_check_digit("15070649C10") + 1) % 10
        );
        assert!(!ssn_de(&wrong), "{wrong}");
        assert!(ssn_us("123-45-6789"));
        assert!(!ssn_us("666-45-6789"));
    }

    #[test]
    fn dates() {
        for ok in [
            "14.03.1987",
            "14.3.87",
            "1987-03-14",
            "03/14/1987",
            "14. März 1987",
            "March 14, 1987",
            "29.02.2000",
        ] {
            assert!(date(ok), "{ok}");
        }
        for bad in [
            "31.04.1987",
            "29.02.1900",
            "14.13.1987",
            "00.01.1990",
            "14.03.1800",
        ] {
            assert!(!date(bad), "{bad}");
        }
    }

    #[test]
    fn plates_and_ips() {
        assert!(kfz_de("HB-MA 412"));
        assert!(kfz_de("M AB 1234"));
        assert!(!kfz_de("QQ-AB 12"));
        assert!(ipv4("203.0.113.24"));
        assert!(!ipv4("10.400.1.1"));
    }
}
