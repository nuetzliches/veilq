//! `scan --explain` (#40): what veilq read and why it decided as it did, on
//! standard error beside the report. It holds the values in clear; the
//! report on standard output stays as it is.

use std::io::Write;

use veilq_core::Reading;
use veilq_core::detect::Verdict;

/// How far below its threshold a candidate is a near miss.
const NEAR: f32 = 0.2;

/// Writes the readings of each file: as text for a person, or with `json`
/// one line `{"path", "reading"}` per reading (unstable). A reading that
/// repeats the one before it (a verify round that found nothing new) is
/// written once.
pub fn write(
    out: &mut dyn Write,
    files: &[(String, Vec<Reading>)],
    json: bool,
) -> std::io::Result<()> {
    if !json {
        writeln!(
            out,
            "explain: what veilq read, with the values in clear; for your own machine only"
        )?;
    }
    for (path, readings) in files {
        if !json {
            writeln!(out, "== {path}")?;
        }
        let mut last: Option<&Reading> = None;
        for reading in readings {
            if last.is_some_and(|l| l == reading) {
                continue;
            }
            last = Some(reading);
            if json {
                serde_json::to_writer(
                    &mut *out,
                    &serde_json::json!({ "path": path, "reading": reading }),
                )?;
                writeln!(out)?;
            } else {
                text(out, reading)?;
            }
        }
    }
    Ok(())
}

fn text(out: &mut dyn Write, reading: &Reading) -> std::io::Result<()> {
    let mut lines = reading.text.lines();
    writeln!(out, "read: {}", lines.next().unwrap_or(""))?;
    for line in lines {
        writeln!(out, "      {line}")?;
    }
    if !reading.context.is_empty() {
        writeln!(out, "  context: {}", reading.context.replace('\n', " | "))?;
    }
    for c in &reading.candidates {
        let near = c.verdict == Verdict::BelowThreshold && c.confidence >= c.threshold - NEAR;
        let compare = if c.confidence >= c.threshold {
            ">="
        } else {
            "<"
        };
        writeln!(
            out,
            "  {:<18} {} {:.2} {compare} {:.2}  {}  {:?}{}{}",
            verdict(c.verdict),
            c.entity,
            c.confidence,
            c.threshold,
            c.detector,
            c.value,
            if c.raised {
                "  (raised: a date beside a name)"
            } else {
                ""
            },
            if near { "  (near miss)" } else { "" },
        )?;
    }
    Ok(())
}

fn verdict(v: Verdict) -> &'static str {
    match v {
        Verdict::Kept => "found",
        Verdict::BelowThreshold => "below threshold",
        Verdict::Allowed => "allowed",
        Verdict::Overlap => "overlapped",
        Verdict::RoleWords => "role words",
        Verdict::Account => "system account",
        Verdict::BrandWord => "brand word",
        Verdict::Placeholder => "placeholder",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veilq_core::{Engine, Explain, Format, Policy};

    #[test]
    fn the_explanation_names_each_verdict_once_per_reading() {
        let explain = Explain::new();
        let engine = Engine::new(&Policy::default(), None, b"s", None)
            .unwrap()
            .with_explain(explain.clone());
        let text = "Frau Mira Albers, geb. 14.03.1987\nC:\\Users\\Public\\Dokumente\n";
        engine.scan_str(text, Format::Text).unwrap();
        let mut out = Vec::new();
        write(&mut out, &[("letter.txt".into(), explain.take())], false).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.starts_with("explain: "), "{out}");
        assert!(out.contains("== letter.txt"), "{out}");
        assert!(out.contains("found              DATE_OF_BIRTH"), "{out}");
        assert!(out.contains("system account     PERSON"), "{out}");

        engine.scan_str(text, Format::Text).unwrap();
        let mut out = Vec::new();
        write(&mut out, &[("letter.txt".into(), explain.take())], true).unwrap();
        for line in String::from_utf8(out).unwrap().lines() {
            let v: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(v["path"], "letter.txt");
            assert!(v["reading"]["text"].is_string());
        }
    }
}
