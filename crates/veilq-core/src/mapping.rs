//! The mapping file (ADR-0018): what a `pseudonymize` or `fake` replacement
//! stood for, so that the owner of a key can put the originals back.
//!
//! It is an age file encrypted to the recipients the policy names
//! (`[mapping] recipients`); there is no unencrypted mapping. Inside, one JSON
//! line per replacement: `{"replacement", "entity", "value"}`. A mapping is
//! the one place outside the input where the values are, so it never goes
//! into a report, an MCP answer or the UI.

use std::collections::BTreeMap;
use std::io::{BufRead, Read, Write};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::Error;

/// One replacement and what it stood for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub replacement: String,
    pub entity: String,
    pub value: String,
}

fn error(message: impl std::fmt::Display) -> Error {
    Error::Mapping(message.to_string())
}

/// Whether `recipient` is an age X25519 recipient (`age1...`).
pub fn check_recipient(recipient: &str) -> Result<(), String> {
    age::x25519::Recipient::from_str(recipient)
        .map(|_| ())
        .map_err(|e| format!("`{recipient}` is not an age recipient: {e}"))
}

/// `entries`, sorted and without duplicates, encrypted to `recipients`.
pub fn encrypt(entries: &[Entry], recipients: &[String]) -> Result<Vec<u8>, Error> {
    let parsed = recipients
        .iter()
        .map(|r| age::x25519::Recipient::from_str(r).map_err(|e| error(format!("{r}: {e}"))))
        .collect::<Result<Vec<_>, _>>()?;
    if parsed.is_empty() {
        return Err(error(
            "the policy names no `[mapping] recipients`; a mapping is never written unencrypted",
        ));
    }
    let mut unique = entries.to_vec();
    unique.sort();
    unique.dedup();
    let mut plain = Vec::new();
    for e in &unique {
        serde_json::to_writer(&mut plain, e).expect("an entry serialises");
        plain.push(b'\n');
    }
    let encryptor =
        age::Encryptor::with_recipients(parsed.iter().map(|r| r as &dyn age::Recipient))
            .map_err(error)?;
    let mut out = Vec::new();
    let mut writer = encryptor.wrap_output(&mut out).map_err(error)?;
    writer.write_all(&plain).map_err(error)?;
    writer.finish().map_err(error)?;
    Ok(out)
}

/// The entries of an encrypted mapping, with the identities of an age
/// identity file (`AGE-SECRET-KEY-...` lines).
pub fn decrypt(data: &[u8], identity_file: &str) -> Result<Vec<Entry>, Error> {
    let identities = age::IdentityFile::from_buffer(identity_file.as_bytes())
        .map_err(error)?
        .into_identities()
        .map_err(error)?;
    if identities.is_empty() {
        return Err(error("the identity file holds no key"));
    }
    let decryptor = age::Decryptor::new(data).map_err(error)?;
    let mut reader = decryptor
        .decrypt(identities.iter().map(|i| i.as_ref() as &dyn age::Identity))
        .map_err(error)?;
    let mut plain = Vec::new();
    reader.read_to_end(&mut plain).map_err(error)?;
    plain
        .lines()
        .map(|l| {
            let l = l.map_err(error)?;
            serde_json::from_str(&l).map_err(error)
        })
        .collect()
}

/// `text` with every replacement of `entries` put back, longest first. A
/// replacement that stands for two different values cannot be put back: an
/// error, not a guess.
pub fn reveal(text: &str, entries: &[Entry]) -> Result<(String, usize), Error> {
    let mut by_replacement: BTreeMap<&str, &str> = BTreeMap::new();
    for e in entries {
        if let Some(other) = by_replacement.insert(&e.replacement, &e.value)
            && other != e.value
        {
            return Err(error(format!(
                "the replacement `{}` ({}) stands for two values; it cannot be put back",
                e.replacement, e.entity
            )));
        }
    }
    let mut order: Vec<(&str, &str)> = by_replacement.into_iter().collect();
    order.sort_by_key(|(r, _)| std::cmp::Reverse(r.len()));
    // One pass, so that a value put back is never read as a replacement.
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    let mut i = 0;
    'outer: while i < text.len() {
        for (r, v) in &order {
            if !r.is_empty() && text[i..].starts_with(r) {
                out.push_str(v);
                i += r.len();
                count += 1;
                continue 'outer;
            }
        }
        let c = text[i..].chars().next().expect("i is on a char boundary");
        out.push(c);
        i += c.len_utf8();
    }
    Ok((out, count))
}

#[cfg(test)]
mod tests {
    use age::secrecy::ExposeSecret;

    use super::*;

    fn entry(r: &str, v: &str) -> Entry {
        Entry {
            replacement: r.into(),
            entity: "PERSON".into(),
            value: v.into(),
        }
    }

    #[test]
    fn a_mapping_round_trips_with_the_key() {
        let key = age::x25519::Identity::generate();
        let recipient = key.to_public().to_string();
        let entries = vec![
            entry("[PERSON:ab12cd34]", "Mira Albers"),
            entry("Anna Bauer", "Lena Vogt"),
        ];
        let data = encrypt(&entries, &[recipient]).unwrap();
        assert!(!String::from_utf8_lossy(&data).contains("Mira"));
        let identity = format!("{}\n", key.to_string().expose_secret());
        let mut back = decrypt(&data, &identity).unwrap();
        back.sort();
        let mut want = entries.clone();
        want.sort();
        assert_eq!(back, want);

        let other = age::x25519::Identity::generate();
        let wrong = format!("{}\n", other.to_string().expose_secret());
        assert!(decrypt(&data, &wrong).is_err());
        assert!(encrypt(&entries, &[]).is_err());
    }

    #[test]
    fn reveal_puts_values_back_once() {
        let entries = vec![
            entry("[PERSON:ab12cd34]", "Mira Albers"),
            entry("Anna", "[PERSON:ab12cd34]"),
        ];
        let (text, n) = reveal("Hallo [PERSON:ab12cd34] und Anna.", &entries).unwrap();
        assert_eq!(text, "Hallo Mira Albers und [PERSON:ab12cd34].");
        assert_eq!(n, 2);
        let clash = vec![entry("X", "a"), entry("X", "b")];
        assert!(reveal("X", &clash).is_err());
    }

    #[test]
    fn recipients_are_checked() {
        let key = age::x25519::Identity::generate();
        assert!(check_recipient(&key.to_public().to_string()).is_ok());
        assert!(check_recipient("age1nope").is_err());
    }
}
