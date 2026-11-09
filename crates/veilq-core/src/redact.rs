//! Redacting text: `detect -> decide -> apply -> verify` (docs/plan.md §3.1).
//!
//! The detectors come in M1. Until then [`redact_text`] fails closed
//! (ADR-0003): a caller that shows or passes on its result must never get the
//! input back as if it had been redacted.

use std::ops::Range;

use crate::{Error, Policy, PseudonymKey, Report};

/// The result of redacting a text.
#[derive(Debug, Clone, PartialEq)]
pub struct Redacted {
    /// The output, the input with every finding replaced.
    pub text: String,
    pub report: Report,
    /// Per finding of the report, in the same order: the byte range of its
    /// replacement in [`Redacted::text`] (empty for `remove`).
    pub output_spans: Vec<Range<usize>>,
}

/// `text` redacted under `policy`. `key` is needed when an action is
/// `pseudonymize`.
pub fn redact_text(
    _text: &str,
    _policy: &Policy,
    _key: Option<&PseudonymKey>,
) -> Result<Redacted, Error> {
    Err(Error::NotBuilt("redaction (detectors come in M1)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fails_closed_until_the_detectors_are_built() {
        let result = redact_text("Erika Mustermann", &Policy::default(), None);
        assert!(matches!(result, Err(Error::NotBuilt(_))));
    }
}
