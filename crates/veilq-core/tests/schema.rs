//! The committed JSON schemas (`schema/`) are what the code produces: a
//! change to a report or policy type shows up as a diff in the schema, and a
//! reviewer sees the contract change (ADR-0002, ADR-0004).
//!
//! `VEILQ_UPDATE_SCHEMA=1 cargo test -p veilq-core --test schema` rewrites
//! them.

use std::path::PathBuf;

fn check(name: &str, schema: serde_json::Value) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schema")
        .join(name);
    let text = serde_json::to_string_pretty(&schema).unwrap() + "\n";
    if std::env::var_os("VEILQ_UPDATE_SCHEMA").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        committed == text,
        "{name} is out of date: run `VEILQ_UPDATE_SCHEMA=1 cargo test -p veilq-core --test schema` and review the diff"
    );
}

#[test]
fn report_schema() {
    check("report.schema.json", veilq_core::Report::json_schema());
}

#[test]
fn policy_schema() {
    check("policy.schema.json", veilq_core::Policy::json_schema());
}
