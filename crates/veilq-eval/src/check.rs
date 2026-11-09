//! Measures veilq against the truth of a corpus (ADR-0013).
//!
//! A piece of personal data in the truth is **covered** when findings in the
//! same place (the same file, JSON value or CSV cell) cover all of its bytes:
//! nothing of it would be left in the output. Recall is covered over all,
//! per type; it is the gate, since a missed value leaks and an extra one
//! costs only readability. **typed** counts the covered ones a finding of the
//! same type overlaps. A finding that overlaps no truth is a false positive.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use veilq_core::{Engine, Format, Location, Policy};

use crate::generate::{TruthFile, truth_path};

#[derive(Debug, Default, Clone, Serialize)]
pub struct Stat {
    pub truth: usize,
    pub covered: usize,
    pub typed: usize,
    pub false_positives: usize,
}

impl Stat {
    pub fn recall(&self) -> f64 {
        if self.truth == 0 {
            1.0
        } else {
            self.covered as f64 / self.truth as f64
        }
    }
}

/// A truth no finding covers, or a finding no truth explains. The corpus is
/// synthetic, so the value is shown: it is what the tuning needs to see.
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub file: String,
    pub entity: String,
    pub location: String,
    pub value: String,
    /// For a false positive: the detector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detector: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Evaluation {
    pub files: usize,
    pub per_entity: BTreeMap<String, Stat>,
    pub misses: Vec<Item>,
    pub false_positives: Vec<Item>,
}

impl Evaluation {
    pub fn total(&self) -> Stat {
        self.per_entity.values().fold(Stat::default(), |a, s| Stat {
            truth: a.truth + s.truth,
            covered: a.covered + s.covered,
            typed: a.typed + s.typed,
            false_positives: a.false_positives + s.false_positives,
        })
    }

    /// Findings that overlap a truth over all findings.
    pub fn precision(&self, findings: usize) -> f64 {
        if findings == 0 {
            1.0
        } else {
            1.0 - self.total().false_positives as f64 / findings as f64
        }
    }
}

/// What the corpus must reach (`eval/gate.toml`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    pub recall: RecallGate,
    pub precision: PrecisionGate,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecallGate {
    pub default: f64,
    #[serde(default)]
    pub entities: BTreeMap<String, f64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrecisionGate {
    pub min: f64,
}

/// Where a location is, as a key two locations of one place share, and its
/// byte range there.
fn place(location: &Location, json_document: bool) -> ((u8, String, u64, u64), (u64, u64)) {
    match location {
        Location::Span {
            part: None,
            start,
            end,
            ..
        } => ((0, String::new(), 0, 0), (*start, *end)),
        Location::Span {
            part: Some(p),
            start,
            end,
            line,
            ..
        } => (
            (1, p.clone(), if json_document { 0 } else { *line }, 0),
            (*start, *end),
        ),
        Location::Cell {
            row,
            column,
            start,
            end,
            ..
        } => ((2, String::new(), *row, *column), (*start, *end)),
        Location::Region { .. } => ((3, String::new(), 0, 0), (0, 0)),
    }
}

fn describe(location: &Location) -> String {
    match location {
        Location::Span {
            part: None,
            line,
            column,
            ..
        } => format!("{line}:{column}"),
        Location::Span {
            part: Some(p),
            line,
            ..
        } => format!("line {line} {p}"),
        Location::Cell { row, column, .. } => format!("row {row}, column {column}"),
        Location::Region { .. } => "region".into(),
    }
}

/// The documents under `dir` (their truth beside them), sorted, those under
/// `only` (a path prefix like `de/csv`) if given.
pub fn documents(dir: &Path, only: Option<&str>) -> anyhow::Result<Vec<PathBuf>> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
        for e in std::fs::read_dir(d)? {
            let p = e?.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "golden") {
                    walk(&p, out)?;
                }
            } else if truth_path(&p).is_file() {
                out.push(p);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, &mut out)?;
    out.sort();
    if let Some(prefix) = only {
        out.retain(|p| relative(dir, p).starts_with(prefix));
    }
    Ok(out)
}

pub fn relative(dir: &Path, p: &Path) -> String {
    p.strip_prefix(dir)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The engine every evaluation and golden test runs: the built-in defaults,
/// a fixed seed.
pub fn engine() -> anyhow::Result<Engine> {
    Ok(Engine::new(&Policy::default(), None, b"veilq eval", None)?)
}

/// Evaluates every document under `dir`; returns the evaluation and the
/// number of findings.
pub fn evaluate(dir: &Path, only: Option<&str>) -> anyhow::Result<(Evaluation, usize)> {
    let engine = engine()?;
    let mut eval = Evaluation::default();
    let mut findings_total = 0;
    for doc in documents(dir, only)? {
        let name = relative(dir, &doc);
        let bytes = std::fs::read(&doc)?;
        let truth: TruthFile = serde_json::from_str(&std::fs::read_to_string(truth_path(&doc))?)?;
        let format = Format::from_path(&doc)?;
        let json_document = format == Format::Json;
        // A container's values are not shown (they are in its parts).
        let content = if format.is_container() {
            String::new()
        } else {
            String::from_utf8(bytes.clone()).map_err(|_| anyhow::anyhow!("{name}: not UTF-8"))?
        };
        let found = engine
            .run_bytes(&bytes, format, false)
            .map_err(|e| anyhow::anyhow!("{name}: {e}"))?
            .1
            .report
            .findings;
        findings_total += found.len();
        eval.files += 1;
        let found_places: Vec<_> = found
            .iter()
            .map(|f| place(&f.location, json_document))
            .collect();
        for t in &truth.findings {
            let (key, (s, e)) = place(&t.location, json_document);
            let stat = eval
                .per_entity
                .entry(t.entity.as_str().to_owned())
                .or_default();
            stat.truth += 1;
            let mut here: Vec<(u64, u64, &str)> = found_places
                .iter()
                .zip(&found)
                .filter(|((k, _), _)| *k == key)
                .map(|((_, r), f)| (r.0, r.1, f.entity.as_str()))
                .collect();
            here.sort();
            let mut at = s;
            for (fs, fe, _) in &here {
                if *fs <= at && *fe > at {
                    at = *fe;
                }
            }
            if at >= e {
                stat.covered += 1;
                if here
                    .iter()
                    .any(|(fs, fe, ent)| *fs < e && *fe > s && *ent == t.entity.as_str())
                {
                    stat.typed += 1;
                }
            } else {
                eval.misses.push(Item {
                    file: name.clone(),
                    entity: t.entity.as_str().to_owned(),
                    location: describe(&t.location),
                    value: value_at(&content, format, &t.location),
                    detector: None,
                });
            }
        }
        for (f, (key, (s, e))) in found.iter().zip(&found_places) {
            let explained = truth.findings.iter().any(|t| {
                let (k, (ts, te)) = place(&t.location, json_document);
                k == *key && ts < *e && te > *s
            });
            if !explained {
                eval.per_entity
                    .entry(f.entity.as_str().to_owned())
                    .or_default()
                    .false_positives += 1;
                eval.false_positives.push(Item {
                    file: name.clone(),
                    entity: f.entity.as_str().to_owned(),
                    location: describe(&f.location),
                    value: value_at(&content, format, &f.location),
                    detector: Some(f.detector.clone()),
                });
            }
        }
    }
    Ok((eval, findings_total))
}

/// The text a location names, for the report of a synthetic corpus.
fn value_at(content: &str, format: Format, location: &Location) -> String {
    let slice = |s: &str, a: u64, b: u64| s.get(a as usize..b as usize).unwrap_or("?").to_owned();
    match location {
        Location::Span {
            part: None,
            start,
            end,
            ..
        } => slice(content, *start, *end),
        Location::Span {
            part: Some(p),
            start,
            end,
            line,
            ..
        } => {
            let doc = if format == Format::Ndjson {
                content.lines().nth(*line as usize - 1).unwrap_or("")
            } else {
                content
            };
            serde_json::from_str::<serde_json::Value>(doc)
                .ok()
                .and_then(|v| {
                    v.pointer(p)
                        .map(|x| x.as_str().map_or(x.to_string(), str::to_owned))
                })
                .map_or("?".into(), |v| slice(&v, *start, *end))
        }
        Location::Cell {
            row,
            column,
            start,
            end,
            ..
        } => {
            let d = if content.lines().next().unwrap_or("").contains(';') {
                ';'
            } else {
                ','
            };
            let line = content.lines().nth(*row as usize - 1).unwrap_or("");
            let mut cells = Vec::new();
            let (mut cell, mut quoted) = (String::new(), false);
            let mut chars = line.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '"' if quoted && chars.peek() == Some(&'"') => {
                        cell.push('"');
                        chars.next();
                    }
                    '"' => quoted = !quoted,
                    c if c == d && !quoted => cells.push(std::mem::take(&mut cell)),
                    c => cell.push(c),
                }
            }
            cells.push(cell);
            cells
                .get(*column as usize - 1)
                .map_or("?".into(), |c| slice(c, *start, *end))
        }
        Location::Region { .. } => "?".into(),
    }
}

/// The gate's verdict: one line per failure, empty when it passes.
pub fn judge(eval: &Evaluation, findings: usize, gate: &Gate) -> Vec<String> {
    let mut failures = Vec::new();
    for (entity, stat) in &eval.per_entity {
        let min = gate
            .recall
            .entities
            .get(entity)
            .copied()
            .unwrap_or(gate.recall.default);
        if stat.truth > 0 && stat.recall() < min {
            failures.push(format!("recall {entity} {:.3} < {min}", stat.recall()));
        }
    }
    let precision = eval.precision(findings);
    if precision < gate.precision.min {
        failures.push(format!("precision {precision:.3} < {}", gate.precision.min));
    }
    failures
}
