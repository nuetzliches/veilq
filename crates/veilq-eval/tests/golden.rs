//! Golden tests (ADR-0013): every document of the committed corpus, redacted
//! with the built-in defaults, against its expected output and report in
//! `eval/golden/`. A change to detection or to how a format is written shows
//! up as a diff a reviewer reads.
//!
//! `VEILQ_UPDATE_GOLDEN=1 cargo test -p veilq-eval --test golden` rewrites
//! them.

use std::path::PathBuf;

use veilq_core::Format;
use veilq_eval::check;

#[test]
fn the_corpus_redacts_as_recorded() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../eval");
    let golden = root.join("golden");
    let update = std::env::var_os("VEILQ_UPDATE_GOLDEN").is_some();
    let engine = check::engine().unwrap();
    let mut stale = Vec::new();
    let docs = check::documents(&root, None).unwrap();
    assert!(!docs.is_empty(), "no corpus under {}", root.display());
    for doc in docs {
        let name = check::relative(&root, &doc);
        let input = std::fs::read(&doc).unwrap();
        let format = Format::from_path(&doc).unwrap();
        let (output, run) = engine
            .run_bytes(&input, format, true)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let report = serde_json::to_string_pretty(&run.report).unwrap() + "\n";
        let mut golden_files = vec![(golden.join(format!("{name}.report.json")), report)];
        // A container's output is a ZIP; its report says what changed in it.
        if !format.is_container() || format == Format::Eml {
            golden_files.push((golden.join(&name), String::from_utf8(output).unwrap()));
        }
        for (path, text) in golden_files {
            if update {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, text).unwrap();
            } else if std::fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n")
                != text.replace("\r\n", "\n")
            {
                stale.push(check::relative(&root, &path));
            }
        }
    }
    assert!(
        stale.is_empty(),
        "out of date: {stale:?}; run `VEILQ_UPDATE_GOLDEN=1 cargo test -p veilq-eval --test golden` and review the diff"
    );
}
