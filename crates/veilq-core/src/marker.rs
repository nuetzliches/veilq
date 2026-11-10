//! A body that carries its verdict (ADR-0024): a ticket's description that a
//! gateway hands to an agent only once veilq has seen it.
//!
//! The first line says what veilq made of the rest: `veilq: cleaned <hash>`
//! (findings were replaced) or `veilq: pass <hash>` (none were found). The
//! hash is the first eight hex digits of SHA-256 over the rest's letters and
//! digits only: a renderer that rewrites Markdown (escapes, list markers,
//! spaces) keeps it, an edit that adds a name or a number does not. A marker
//! whose hash does not match, or that has none, counts for nothing: the body
//! is read again.
//!
//! A body whose first line starts with one of the policy's `[markers]
//! exempt` prefixes (an agent's technical note) is handed on as it is, with
//! no marker. Only the first line: a customer's text with a note appended
//! is read.
//!
//! veilq cannot read an image behind a URL; an image in the body
//! (`![..](..)`, Linear's `<linear-image>`) is replaced by [`IMAGE_NOTE`] and
//! counts as a finding of the type `IMAGE`.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::finding::{EntityType, Finding, Location, Report};
use crate::{Engine, Error, Format};

/// What stands where an image was.
pub const IMAGE_NOTE: &str = "[image removed: not checked by veilq]";

static MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^veilq:\s*(cleaned|pass)(?:\s+([0-9a-f]{8}))?\s*$").expect("a valid regex")
});
static IMAGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<linear-image\b.*?</linear-image>|!\[[^\]]*\]\([^)]*\)")
        .expect("a valid regex")
});

/// What veilq made of a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Its marker matches: handed on as it is.
    Kept,
    /// Its first line is an exempt prefix: handed on as it is.
    Exempt,
    /// Findings were replaced; `write` holds the new body.
    Cleaned,
    /// Nothing was found; `write` holds the body with its marker.
    Pass,
}

/// The verdict on a body.
#[derive(Debug, Clone, Serialize)]
pub struct Marked {
    pub state: State,
    /// What may be handed to the agent.
    pub text: String,
    /// What the gateway writes back, when it differs from the body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write: Option<String>,
    pub report: Report,
}

/// The hash of a marker: SHA-256 over the letters and digits of `text`.
pub fn hash(text: &str) -> String {
    let mut h = Sha256::new();
    for c in text.chars().filter(|c| c.is_alphanumeric()) {
        let mut buf = [0u8; 4];
        h.update(c.encode_utf8(&mut buf).as_bytes());
    }
    h.finalize()[..4]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The first line and the rest after it.
fn split_first(body: &str) -> (&str, &str) {
    match body.find('\n') {
        Some(i) => (body[..i].trim_end_matches('\r'), &body[i + 1..]),
        None => (body, ""),
    }
}

/// veilq's verdict on `body`, under `exempt` (the policy's prefixes).
pub fn mark(engine: &Engine, body: &str) -> Result<Marked, Error> {
    let exempt = &engine.policy().markers.exempt;
    let empty = |state, text: &str| Marked {
        state,
        text: text.to_owned(),
        write: None,
        report: Report::new(Vec::new()),
    };
    let (first, rest) = split_first(body);
    if let Some(m) = MARKER.captures(first.trim())
        && m.get(2).is_some_and(|h| h.as_str() == hash(rest))
    {
        return Ok(empty(State::Kept, body));
    }
    let first_trimmed = first.trim_start();
    if exempt
        .iter()
        .any(|p| !p.is_empty() && first_trimmed.starts_with(p.as_str()))
    {
        return Ok(empty(State::Exempt, body));
    }
    // A stale marker is not part of the text.
    let text = if first.trim_start().starts_with("veilq:") {
        rest
    } else {
        body
    };
    let images: Vec<(usize, usize)> = IMAGE
        .find_iter(text)
        .map(|m| (m.start(), m.end()))
        .collect();
    let without_images = IMAGE.replace_all(text, IMAGE_NOTE);
    let redacted = engine.redact_str(&without_images, Format::Markdown)?;
    let mut findings = redacted.report.findings;
    // An image's place is in the body as it came (its text findings are in
    // the body without its images).
    for &(start, end) in &images {
        let before = &text[..start];
        findings.push(Finding {
            entity: EntityType::new("IMAGE").expect("a valid type"),
            location: Location::Span {
                part: Some("images".into()),
                start: start as u64,
                end: end as u64,
                line: before.matches('\n').count() as u64 + 1,
                column: before[before.rfind('\n').map_or(0, |i| i + 1)..]
                    .chars()
                    .count() as u64
                    + 1,
            },
            confidence: 1.0,
            detector: "marker".into(),
            pseudonym: None,
        });
    }
    let mut reasons = redacted.report.review_reasons;
    if !images.is_empty() {
        reasons.push(format!(
            "{} image(s) removed: veilq does not read an image behind a URL",
            images.len()
        ));
    }
    let (state, new) = if findings.is_empty() {
        (State::Pass, text.to_owned())
    } else {
        (State::Cleaned, redacted.text)
    };
    let label = if state == State::Pass {
        "pass"
    } else {
        "cleaned"
    };
    let write = format!("veilq: {label} {}\n{new}", hash(&new));
    let mut report = Report::new(findings);
    report.review_required = redacted.report.review_required || !reasons.is_empty();
    report.review_reasons = reasons;
    Ok(Marked {
        state,
        text: write.clone(),
        write: Some(write),
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Policy;

    fn engine() -> Engine {
        let policy =
            Policy::parse("version = 1\n[markers]\nexempt = [\"> 🤖 Zu Dokumentationszwecken\"]\n")
                .unwrap();
        Engine::new(&policy, None, b"seed", None).unwrap()
    }

    const BODY: &str = "Bitte Frau Mira Albers zurückrufen: 0421 5550173.\n\n![shot](https://uploads.example/a.png)\n";

    #[test]
    fn a_body_is_cleaned_once_and_kept_after() {
        let e = engine();
        let first = mark(&e, BODY).unwrap();
        assert_eq!(first.state, State::Cleaned);
        let write = first.write.unwrap();
        assert!(write.starts_with("veilq: cleaned "), "{write}");
        assert!(
            write.contains(r"\<PERSON\>") && write.contains(IMAGE_NOTE),
            "{write}"
        );
        assert!(
            !write.contains("Albers") && !write.contains("5550173") && !write.contains("a.png")
        );
        let entities: Vec<&str> = first
            .report
            .findings
            .iter()
            .map(|f| f.entity.as_str())
            .collect();
        assert_eq!(entities, ["PERSON", "PHONE_NUMBER", "IMAGE"]);
        // Read again: kept, also after a renderer rewrote the Markdown.
        assert_eq!(mark(&e, &write).unwrap().state, State::Kept);
        let rendered = write
            .replace(r"\<", "<")
            .replace(r"\>", ">")
            .replace("\n\n", "\n");
        assert_eq!(mark(&e, &rendered).unwrap().state, State::Kept);
        // Something added under the marker: read again.
        let edited = format!("{write}Neue Nummer: 0421 5550199\n");
        let again = mark(&e, &edited).unwrap();
        assert_eq!(again.state, State::Cleaned);
        assert!(!again.write.unwrap().contains("5550199"));
    }

    #[test]
    fn a_marker_without_its_hash_counts_for_nothing() {
        let e = engine();
        for body in [
            "veilq: pass\nRückruf unter 0421 5550173\n",
            "veilq: cleaned 00000000\nRückruf unter 0421 5550173\n",
        ] {
            let m = mark(&e, body).unwrap();
            assert_eq!(m.state, State::Cleaned, "{body}");
            let write = m.write.unwrap();
            assert_eq!(write.matches("veilq:").count(), 1, "{write}");
            assert!(!write.contains("5550173"), "{write}");
        }
    }

    #[test]
    fn nothing_found_is_a_pass_with_its_marker() {
        let m = mark(&engine(), "Der Endpunkt antwortet mit 500.\n").unwrap();
        assert_eq!(m.state, State::Pass);
        let write = m.write.unwrap();
        assert_eq!(
            write,
            format!(
                "veilq: pass {}\nDer Endpunkt antwortet mit 500.\n",
                hash("Der Endpunkt antwortet mit 500.\n")
            )
        );
        assert_eq!(mark(&engine(), &write).unwrap().state, State::Kept);
    }

    #[test]
    fn only_a_first_line_note_is_exempt() {
        let e = engine();
        let note =
            "> 🤖 Zu Dokumentationszwecken - technische Notiz.\n\nMira Albers, 0421 5550173\n";
        let m = mark(&e, note).unwrap();
        assert_eq!(
            (m.state, m.write.as_deref(), m.text.as_str()),
            (State::Exempt, None, note)
        );
        // The note appended to a customer's text exempts nothing.
        let appended = format!("Mira Albers, 0421 5550173\n{note}");
        assert_eq!(mark(&e, &appended).unwrap().state, State::Cleaned);
        // Without the policy's prefix, a note is read like any body.
        let plain = Engine::new(&Policy::default(), None, b"seed", None).unwrap();
        assert_eq!(mark(&plain, note).unwrap().state, State::Cleaned);
    }
}
