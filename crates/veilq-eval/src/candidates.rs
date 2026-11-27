//! Every candidate of a corpus with its label, for measuring a decider's
//! models outside veilq (#95, #35).
//!
//! One JSON line per candidate the detectors proposed, kept or not: what
//! `scan --explain` records (value, context, detector, confidence, the
//! policy's verdict) and the text around it, labelled against the truth.
//!
//! The label is by value: a candidate is personal data when its value and a
//! value of the document's truth contain one another (the shorter at least
//! [`MIN_OVERLAP`] characters). The readings hold no document locations, so
//! this is the join that holds across formats; on a synthetic corpus a
//! value that matches by chance is rare. Only documents whose truth values
//! can be read are written: text, CSV, JSON and NDJSON, and with OCR the
//! pictures, whose truth holds the value as drawn (#175). A picture's
//! candidate also matches a value OCR read one letter off in six
//! ([`near`]). Containers are skipped and counted.

use std::io::Write;
use std::path::Path;

use serde::Serialize;
use veilq_core::{Explain, Format};

use crate::check::{documents, engine_for, relative, value_at};
use crate::generate::{TruthFile, truth_path};

/// Characters of the reading's text kept on each side of a candidate.
pub const AROUND: usize = 100;

/// The shorter of two values must have this many characters to count as
/// contained in the other.
pub const MIN_OVERLAP: usize = 3;

/// One candidate and its label.
#[derive(Debug, Serialize)]
pub struct Line<'a> {
    /// The document, relative to the corpus.
    pub file: &'a str,
    /// The document's template: its path without the number (`de/text/letter`),
    /// for splits that hold a whole template out.
    pub family: &'a str,
    pub format: &'a str,
    /// The reading's index in the document, in the order it was read.
    pub reading: usize,
    /// A verify round's reading.
    pub verify: bool,
    /// What the reading was read with as its context (a column's name).
    pub context: &'a str,
    pub before: &'a str,
    pub after: &'a str,
    #[serde(flatten)]
    pub candidate: &'a veilq_core::Explained,
    /// The label: the value is personal data by the truth.
    pub personal: bool,
    /// The truth's type of the value it matched, when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truth_entity: Option<&'a str>,
}

/// What a run wrote.
#[derive(Debug, Default)]
pub struct Written {
    pub documents: usize,
    pub candidates: usize,
    pub personal: usize,
    /// Pictures, containers and documents whose truth could not be read.
    pub skipped: usize,
}

/// The template of a document: its relative path without the trailing
/// `-<n>` and the extension.
pub fn family(name: &str) -> &str {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    match stem.rsplit_once('-') {
        Some((head, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => stem,
    }
}

/// Whether two values contain one another, the shorter long enough.
pub fn matches(candidate: &str, truth: &str) -> bool {
    let (c, t) = (candidate.trim(), truth.trim());
    if c.is_empty() || t.is_empty() {
        return false;
    }
    if c == t {
        return true;
    }
    let shorter = c.chars().count().min(t.chars().count());
    shorter >= MIN_OVERLAP && (t.contains(c) || c.contains(t))
}

/// The characters to change, add or remove, or neighbours to swap, to turn
/// `a` into `b` (optimal string alignment).
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// Whether a candidate read by OCR is a value of the truth: as [`matches`],
/// or a word of it read at most one letter off in six (`Wiekbe` for
/// `Wiebke`), the word at least five letters long.
pub fn near(candidate: &str, truth: &str) -> bool {
    if matches(candidate, truth) {
        return true;
    }
    let (c, t) = (candidate.trim().to_lowercase(), truth.to_lowercase());
    let n = c.chars().count();
    n >= 5
        && t.split_whitespace()
            .chain(std::iter::once(t.trim()))
            .any(|w| distance(&c, w) <= n / 6)
}

/// At most `n` characters of `s` from its end (`tail`) or its start.
fn clip(s: &str, n: usize, tail: bool) -> &str {
    let count = s.chars().count();
    if count <= n {
        return s;
    }
    if tail {
        let (at, _) = s.char_indices().nth(count - n).expect("within the text");
        &s[at..]
    } else {
        let (at, _) = s.char_indices().nth(n).expect("within the text");
        &s[..at]
    }
}

/// Writes every candidate of the documents under `dir` to `out`.
pub fn write(
    dir: &Path,
    only: Option<&str>,
    ner: bool,
    ocr: bool,
    out: &mut impl Write,
) -> anyhow::Result<Written> {
    let explain = Explain::new();
    let engine = engine_for(ner, ocr)?.with_explain(explain.clone());
    let mut written = Written::default();
    for doc in documents(dir, only)? {
        let name = relative(dir, &doc);
        let format = Format::from_path(&doc)?;
        let picture = format.is_picture();
        // `is_container` holds for every format that is no plain text,
        // pictures too.
        if (format.is_container() && !picture) || (picture && !ocr) {
            written.skipped += 1;
            continue;
        }
        let bytes = std::fs::read(&doc)?;
        let truth: TruthFile = serde_json::from_str(&std::fs::read_to_string(truth_path(&doc))?)?;
        // A picture's values come with its truth (a picture's metadata is no
        // text and has none); a text's are read back from it.
        let values: Vec<(String, &str)> = if picture {
            truth
                .findings
                .iter()
                .filter(|t| t.entity.as_str() != "METADATA")
                .map(|t| {
                    (
                        t.value.clone().unwrap_or_else(|| "?".into()),
                        t.entity.as_str(),
                    )
                })
                .filter(|(v, _)| v != "?")
                .collect()
        } else {
            let content = String::from_utf8(bytes.clone())
                .map_err(|_| anyhow::anyhow!("{name}: not UTF-8"))?;
            truth
                .findings
                .iter()
                .map(|t| (value_at(&content, format, &t.location), t.entity.as_str()))
                .filter(|(v, _)| v != "?")
                .collect()
        };
        let expected = truth
            .findings
            .iter()
            .filter(|t| !(picture && t.entity.as_str() == "METADATA"))
            .count();
        if values.len() < expected {
            written.skipped += 1;
            continue;
        }
        let same = if picture { near } else { matches };
        explain.take();
        engine
            .run_bytes(&bytes, format, false)
            .map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
        written.documents += 1;
        for (i, reading) in explain.take().iter().enumerate() {
            for c in &reading.candidates {
                let truth_entity = values
                    .iter()
                    .find(|(v, _)| same(&c.value, v))
                    .map(|(_, e)| *e);
                let line = Line {
                    file: &name,
                    family: family(&name),
                    format: format.name(),
                    reading: i,
                    verify: reading.verify,
                    context: &reading.context,
                    before: clip(&reading.text[..c.start], AROUND, true),
                    after: clip(&reading.text[c.end..], AROUND, false),
                    candidate: c,
                    personal: truth_entity.is_some(),
                    truth_entity,
                };
                serde_json::to_writer(&mut *out, &line)?;
                out.write_all(b"\n")?;
                written.candidates += 1;
                written.personal += usize::from(line.personal);
            }
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_family_is_the_path_without_its_number() {
        assert_eq!(family("de/text/letter-12.txt"), "de/text/letter");
        assert_eq!(family("de/csv/customers-1.csv"), "de/csv/customers");
        assert_eq!(family("en/text/notes.txt"), "en/text/notes");
        assert_eq!(family("de/text/two-words.txt"), "de/text/two-words");
    }

    #[test]
    fn values_match_when_one_contains_the_other() {
        assert!(matches("Lena Brandt", "Lena Brandt"));
        assert!(matches("Brandt", "Lena Brandt"));
        assert!(matches("Frau Lena Brandt", "Lena Brandt"));
        assert!(!matches("Le", "Lena Brandt"), "too short to count");
        assert!(!matches("Berlin", "Lena Brandt"));
        assert!(!matches(" ", "Lena Brandt"));
    }

    #[test]
    fn a_value_read_by_ocr_matches_one_letter_off_in_six() {
        assert!(near("Wiekbe", "Wiebke Albers"), "two letters swapped");
        assert!(near("Smartphone von Wiebke", "Wiebke"));
        assert!(!near("Bereit", "Wiebke Albers"));
        assert!(!near("Jan", "Jon Kern"), "too short to forgive a letter");
        assert!(!near("Corvex", "Carvax"), "two letters off in six");
    }

    #[test]
    fn a_pictures_truth_holds_its_values() {
        let dir = std::env::temp_dir().join(format!("veilq-truth-values-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::generate::write(&dir, 1).unwrap();
        let truth: TruthFile = serde_json::from_str(
            &std::fs::read_to_string(dir.join("de/png/ui-1.png.truth.json")).unwrap(),
        )
        .unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!truth.findings.is_empty());
        assert!(
            truth
                .findings
                .iter()
                .all(|t| t.value.as_deref().is_some_and(|v| !v.is_empty()))
        );
    }

    #[test]
    fn clipping_keeps_whole_characters() {
        assert_eq!(clip("Grüße aus Köln", 4, true), "Köln");
        assert_eq!(clip("Grüße aus Köln", 5, false), "Grüße");
        assert_eq!(clip("kurz", 100, false), "kurz");
    }

    #[test]
    fn every_candidate_of_a_corpus_is_written_with_its_label() {
        let dir = std::env::temp_dir().join(format!("veilq-candidates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::generate::write(&dir, 3).unwrap();
        let mut out = Vec::new();
        let written = write(&dir, Some("de/"), false, false, &mut out).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(written.documents > 0 && written.skipped > 0);
        let lines: Vec<serde_json::Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), written.candidates);
        assert!(written.personal > 0, "the corpus has personal data");
        for l in &lines {
            for key in ["file", "family", "value", "entity", "detector", "verdict"] {
                assert!(l[key].is_string(), "{key} in {l}");
            }
            assert!(l["confidence"].is_number() && l["personal"].is_boolean());
        }
        // A kept finding that is personal data, and its type from the truth.
        assert!(lines.iter().any(|l| l["verdict"] == "kept"
            && l["personal"] == true
            && l["truth_entity"].is_string()));
        // The hand rules come with it as named features (#35).
        assert!(
            lines
                .iter()
                .any(|l| l["features"].as_array().is_some_and(|f| !f.is_empty()))
        );
    }
}
