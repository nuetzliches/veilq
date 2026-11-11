//! `veilq-eval`: the synthetic corpus of veilq (ADR-0013).
//!
//! - `generate --seed <n> --out <dir>` writes a corpus with its truth.
//! - `recording --seed <n> --out <dir>` writes a long screen recording.
//! - `check <dir>` measures veilq against it, and against corpora of
//!   held-out seeds generated on the fly; exit 1 when the gate
//!   (`eval/gate.toml`) fails.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use veilq_eval::{check, generate};

#[derive(Parser)]
#[command(name = "veilq-eval")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Write the corpus of a seed.
    Generate {
        #[arg(long)]
        seed: u64,
        #[arg(long)]
        out: PathBuf,
    },
    /// Write a long screen recording (1920x1080, about 130 frames) with its
    /// truth, outside the corpus: `check <out> --ocr` measures it.
    Recording {
        #[arg(long)]
        seed: u64,
        #[arg(long)]
        out: PathBuf,
    },
    /// Measure recall and precision against a corpus.
    Check {
        /// The corpus, `eval/` in the repository.
        dir: PathBuf,
        /// Only documents under this path in the corpus (`de/csv`).
        #[arg(long)]
        only: Option<String>,
        /// Seeds of corpora generated for this check only: decided on, never
        /// tuned on.
        #[arg(long, value_delimiter = ',')]
        held_out: Vec<u64>,
        /// The gate; default `gate.toml` in the corpus.
        #[arg(long)]
        gate: Option<PathBuf>,
        /// The evaluation as JSON.
        #[arg(long)]
        json: bool,
        /// Run the installed NER model too (`veilq models fetch ner`).
        #[arg(long)]
        ner: bool,
        /// Read the pictures too, with the installed OCR and PDFium
        /// (`veilq models fetch ocr`, `veilq models fetch pdf`); without
        /// it they are skipped.
        #[arg(long)]
        ocr: bool,
        /// How many misses and false positives to list.
        #[arg(long, default_value_t = 40)]
        list: usize,
        /// Corpora checked at the same time, each with its own engine. With
        /// models it pays only where cores are left over: a model runs on
        /// every core itself (on a 4-core CI runner, 2 jobs took as long as
        /// 1).
        #[arg(long, default_value_t = 1)]
        jobs: usize,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<bool> {
    match cli.command {
        Command::Generate { seed, out } => {
            let n = generate::write(&out, seed)?;
            println!("{n} documents written to {}", out.display());
            Ok(true)
        }
        Command::Recording { seed, out } => {
            generate::write_recording(&out, seed)?;
            println!("a recording written to {}", out.display());
            Ok(true)
        }
        Command::Check {
            dir,
            only,
            held_out,
            gate,
            json,
            list,
            ner,
            ocr,
            jobs,
        } => {
            let gate_path = gate.unwrap_or_else(|| dir.join("gate.toml"));
            let gate: check::Gate = toml::from_str(&std::fs::read_to_string(&gate_path)?)
                .map_err(|e| anyhow::anyhow!("{}: {e}", gate_path.display()))?;
            let mut ok = true;
            let mut runs = vec![("corpus".to_owned(), dir.clone())];
            for seed in &held_out {
                let tmp =
                    std::env::temp_dir().join(format!("veilq-eval-{seed}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&tmp);
                generate::write(&tmp, *seed)?;
                runs.push((format!("held-out seed {seed}"), tmp));
            }
            // The corpora are checked side by side, the results printed in
            // their order.
            let jobs = jobs.clamp(1, runs.len());
            let mut results = Vec::with_capacity(runs.len());
            for chunk in runs.chunks(jobs) {
                let done: Vec<_> = std::thread::scope(|s| {
                    let handles: Vec<_> = chunk
                        .iter()
                        .map(|(_, d)| s.spawn(|| check::evaluate(d, only.as_deref(), ner, ocr)))
                        .collect();
                    handles
                        .into_iter()
                        .map(|h| h.join().expect("an evaluation panicked"))
                        .collect()
                });
                results.extend(done);
            }
            for ((label, d), result) in runs.iter().zip(results) {
                let (eval, findings) = result?;
                let failures = check::judge(&eval, findings, &gate);
                if json {
                    println!(
                        "{}",
                        serde_json::to_string(&serde_json::json!({
                            "label": label, "evaluation": eval, "findings": findings, "failures": failures,
                        }))?
                    );
                } else {
                    print_table(label, &eval, findings, list);
                    for f in &failures {
                        println!("GATE FAILED: {f}");
                    }
                }
                ok &= failures.is_empty();
                if d != &dir {
                    let _ = std::fs::remove_dir_all(d);
                }
            }
            Ok(ok)
        }
    }
}

fn print_table(label: &str, eval: &check::Evaluation, findings: usize, list: usize) {
    println!("== {label}: {} files, {findings} findings", eval.files);
    println!(
        "{:<24} {:>6} {:>8} {:>8} {:>7} {:>5}",
        "entity", "truth", "covered", "recall", "typed", "fp"
    );
    for (e, s) in &eval.per_entity {
        println!(
            "{e:<24} {:>6} {:>8} {:>8.3} {:>7} {:>5}",
            s.truth,
            s.covered,
            s.recall(),
            s.typed,
            s.false_positives
        );
    }
    let t = eval.total();
    println!(
        "{:<24} {:>6} {:>8} {:>8.3} {:>7} {:>5}   precision {:.3}",
        "all",
        t.truth,
        t.covered,
        t.recall(),
        t.typed,
        t.false_positives,
        eval.precision(findings)
    );
    for m in eval.misses.iter().take(list) {
        println!(
            "  miss {} {} at {}: {:?}",
            m.file, m.entity, m.location, m.value
        );
    }
    for f in eval.false_positives.iter().take(list) {
        println!(
            "  false positive {} {} at {} ({}): {:?}",
            f.file,
            f.entity,
            f.location,
            f.detector.as_deref().unwrap_or(""),
            f.value
        );
    }
}
