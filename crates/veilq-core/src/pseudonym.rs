//! Pseudonyms: `[TYPE:hex8]`, where hex8 is the first four bytes of
//! HMAC-SHA256(key, normalised value) in lowercase hex (ADR-0001).
//!
//! The shape matches the one powerbrain writes, so tools that parse it read
//! both; the values do not, since powerbrain hashes `salt:text` unkeyed.
//! Eight hex characters are 32 bits: within one key, two distinct values of
//! one type collide with a probability of about n²/2³³ (1 % at ~9,000
//! values). A token is a label for a reader, not a unique identifier; the
//! mapping file (M2) resolves it.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;

use crate::Error;
use crate::finding::EntityType;

/// A key below this length is refused: a short key makes the tokens of a
/// small value space (birth dates, postcodes) easy to brute-force.
pub const MIN_KEY_LEN: usize = 16;

/// The secret that makes pseudonyms stable within a project and useless
/// outside it. Its source is open (plan O-3); the caller hands the bytes in.
#[derive(Clone)]
pub struct PseudonymKey(Vec<u8>);

impl PseudonymKey {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, Error> {
        let bytes = bytes.into();
        if bytes.len() < MIN_KEY_LEN {
            return Err(Error::KeyTooShort {
                min: MIN_KEY_LEN,
                got: bytes.len(),
            });
        }
        Ok(Self(bytes))
    }

    /// The pseudonym of `value` as an entity of type `entity`.
    pub fn pseudonym(&self, entity: &EntityType, value: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC takes any key length");
        // The type is part of the input: the same digits as a phone number
        // and as an account number give different tokens.
        mac.update(entity.as_str().as_bytes());
        mac.update(&[0]);
        mac.update(normalise(entity, value).as_bytes());
        let tag = mac.finalize().into_bytes();
        let hex: String = tag[..4].iter().map(|b| format!("{b:02x}")).collect();
        format!("[{}:{hex}]", entity.as_str())
    }
}

impl PseudonymKey {
    /// The key the policy names (`[pseudonymize] key_env`), if the variable
    /// is set. A policy that pseudonymizes without it fails when the run
    /// starts ([`crate::Engine::new`]).
    pub fn from_policy_env(policy: &crate::Policy) -> Result<Option<Self>, Error> {
        match &policy.pseudonymize {
            Some(p) => match std::env::var(&p.key_env) {
                Ok(k) => Self::new(k.into_bytes()).map(Some),
                Err(_) => Ok(None),
            },
            None => Ok(None),
        }
    }

    /// The seed of `fake` under this key: stand-ins stable across runs with
    /// the same key, unrelated to the pseudonyms.
    pub fn fake_seed(&self) -> Vec<u8> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC takes any key length");
        mac.update(b"veilq fake seed");
        mac.finalize().into_bytes().to_vec()
    }
}

impl std::fmt::Debug for PseudonymKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PseudonymKey(..)")
    }
}

/// Unicode NFC, case folded, whitespace collapsed: how allow-list values are
/// compared.
pub(crate) fn normalise_plain(value: &str) -> String {
    let nfc: String = value.nfc().collect::<String>().to_lowercase();
    nfc.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Spellings of one value that mean the same thing give the same token:
/// Unicode NFC, case folded, whitespace collapsed; for identifiers whose
/// separators carry no meaning (IBAN, phone numbers), separators removed.
fn normalise(entity: &EntityType, value: &str) -> String {
    let nfc: String = value.nfc().collect::<String>().to_lowercase();
    match entity.as_str() {
        "IBAN_CODE" | "PHONE_NUMBER" | "CREDIT_CARD" => nfc
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '+')
            .collect(),
        _ => nfc.split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> PseudonymKey {
        PseudonymKey::new(b"0123456789abcdef".to_vec()).unwrap()
    }

    fn entity(name: &str) -> EntityType {
        EntityType::new(name).unwrap()
    }

    #[test]
    fn shape_is_type_and_eight_hex() {
        let p = key().pseudonym(&entity("PERSON"), "Erika Mustermann");
        let re = regex::Regex::new(r"^\[PERSON:[0-9a-f]{8}\]$").unwrap();
        assert!(re.is_match(&p), "{p}");
    }

    #[test]
    fn spellings_of_one_value_agree() {
        let k = key();
        let person = entity("PERSON");
        assert_eq!(
            k.pseudonym(&person, "Erika  Mustermann"),
            k.pseudonym(&person, "erika mustermann")
        );
        let iban = entity("IBAN_CODE");
        assert_eq!(
            k.pseudonym(&iban, "DE02 1203 0000 0000 2020 51"),
            k.pseudonym(&iban, "de02120300000000202051")
        );
        // NFC: a precomposed and a decomposed umlaut are one name.
        assert_eq!(
            k.pseudonym(&person, "M\u{00fc}ller"),
            k.pseudonym(&person, "Mu\u{0308}ller")
        );
    }

    #[test]
    fn type_and_key_change_the_token() {
        let k = key();
        assert_ne!(
            k.pseudonym(&entity("PHONE_NUMBER"), "030 1234567"),
            k.pseudonym(&entity("ACCOUNT_NUMBER"), "030 1234567")
        );
        let other = PseudonymKey::new(b"fedcba9876543210".to_vec()).unwrap();
        let person = entity("PERSON");
        assert_ne!(
            k.pseudonym(&person, "Erika Mustermann"),
            other.pseudonym(&person, "Erika Mustermann")
        );
    }

    #[test]
    fn short_key_is_refused() {
        assert!(matches!(
            PseudonymKey::new(b"short".to_vec()),
            Err(Error::KeyTooShort { .. })
        ));
    }

    #[test]
    fn debug_hides_the_key() {
        assert_eq!(format!("{:?}", key()), "PseudonymKey(..)");
    }
}
