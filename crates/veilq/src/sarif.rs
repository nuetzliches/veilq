//! SARIF 2.1.0 for code scanning. A result names the type, the detector and
//! where; never the value (ADR-0002): no snippet, no region text.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use veilq_core::{Location, Report};

/// Where a finding is, for a person: `3:14`, `row 2, column 3`, `4:9 at /user/email`.
pub fn describe(location: &Location) -> String {
    match location {
        Location::Span {
            part, line, column, ..
        } => match part {
            Some(p) => format!("{line}:{column} at {p}"),
            None => format!("{line}:{column}"),
        },
        Location::Cell {
            sheet, row, column, ..
        } => match sheet {
            Some(s) => format!("{s}, row {row}, column {column}"),
            None => format!("row {row}, column {column}"),
        },
        Location::Region {
            part,
            frame,
            x,
            y,
            width,
            height,
        } => {
            let at = match frame {
                Some(f) => format!("frame {f}, {width}x{height} at {x},{y}"),
                None => format!("{width}x{height} at {x},{y}"),
            };
            match part {
                Some(p) => format!("{at} in {p}"),
                None => at,
            }
        }
    }
}

pub fn sarif(results: &[(String, Report)]) -> Value {
    let entities: BTreeSet<&str> = results
        .iter()
        .flat_map(|(_, r)| r.findings.iter().map(|f| f.entity.as_str()))
        .collect();
    let rules: Vec<Value> = entities
        .iter()
        .map(|e| {
            json!({
                "id": e,
                "name": e,
                "shortDescription": { "text": format!("Personal data: {e}") },
                "defaultConfiguration": { "level": "error" },
            })
        })
        .collect();
    let items: Vec<Value> = results
        .iter()
        .flat_map(|(path, report)| {
            report.findings.iter().map(move |f| {
                // A cell's row stands in for its line: close for CSV, where a
                // record is one line unless a field holds a line break.
                let (line, column) = match &f.location {
                    Location::Span { line, column, .. } => (*line, *column),
                    Location::Cell { row, .. } => (*row, 1),
                    Location::Region { .. } => (1, 1),
                };
                json!({
                    "ruleId": f.entity.as_str(),
                    "level": "error",
                    "message": { "text": format!(
                        "{} at {} (detector {}, confidence {:.2})",
                        f.entity.as_str(), describe(&f.location), f.detector, f.confidence
                    ) },
                    "locations": [{
                        "physicalLocation": {
                            "artifactLocation": { "uri": path.replace('\\', "/") },
                            "region": { "startLine": line, "startColumn": column },
                        }
                    }],
                })
            })
        })
        .collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": { "driver": {
                "name": "veilq",
                "version": env!("CARGO_PKG_VERSION"),
                "informationUri": "https://github.com/nuetzliches/veilq",
                "rules": rules,
            } },
            "results": items,
        }],
    })
}
