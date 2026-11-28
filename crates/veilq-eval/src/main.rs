//! `veilq-eval`: the synthetic corpus of veilq (ADR-0013).
//!
//! - `generate --seed <n> --out <dir>` writes a corpus with its truth.
//! - `recording --seed <n> --out <dir>` writes a long screen recording.
//! - `check <dir>` measures veilq against it, and against corpora of
//!   held-out seeds generated on the fly; exit 1 when the gate
//!   (`eval/gate.toml`) fails.
//! - `candidates <dir> --out <file>` writes every candidate with its label,
//!   one JSON line each, for measuring a decider's models (#95).
//! - `ablate <dir>` reads the corpus once per hand-written patch with it
//!   off (#161).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use veilq_eval::{candidates, check, generate};

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
        /// Measure what a redaction finds, not a scan (#108): a picture's
        /// faces and signatures are filled as a redaction fills them, and
        /// it is read again until a round finds nothing new.
        #[arg(long)]
        redact: bool,
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
    /// Measure what each hand-written patch of the detection is worth
    /// (#161): the corpus once with every patch on, then once per patch with
    /// it off. The patches are those of `patches.toml` in the corpus.
    #[cfg(feature = "ablation")]
    Ablate {
        /// The corpus, `eval/` in the repository.
        dir: PathBuf,
        /// Only these patches (ids of `patches.toml`); default all.
        #[arg(long, value_delimiter = ',')]
        patch: Vec<String>,
        /// Only documents under this path in the corpus (`de/png`).
        #[arg(long)]
        only: Option<String>,
        /// Seeds of corpora generated for this run, read beside the corpus.
        #[arg(long, value_delimiter = ',')]
        held_out: Vec<u64>,
        /// Run the installed NER model too.
        #[arg(long)]
        ner: bool,
        /// Read the pictures too, with the installed OCR and PDFium.
        #[arg(long)]
        ocr: bool,
        /// Measure what a redaction finds, as `check --redact`: its verify
        /// rounds read veilq's own output (ADR-0071).
        #[arg(long)]
        redact: bool,
        /// The effects as JSON, one line per patch.
        #[arg(long)]
        json: bool,
    },
    /// Write every candidate the detectors proposed, kept or not, with its
    /// label from the truth: one JSON line each (text, CSV, JSON, NDJSON).
    Candidates {
        /// The corpus: `eval/`, or one written by `generate`.
        dir: PathBuf,
        /// Only documents under this path in the corpus (`de/csv`).
        #[arg(long)]
        only: Option<String>,
        /// Run the installed NER model too (`veilq models fetch ner`).
        #[arg(long)]
        ner: bool,
        /// Read the pictures too, with the installed OCR (`veilq models fetch
        /// ocr`), labelled by the values their truth holds (#175).
        #[arg(long)]
        ocr: bool,
        /// The JSONL file to write.
        #[arg(long)]
        out: PathBuf,
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
        #[cfg(feature = "ablation")]
        Command::Ablate {
            dir,
            patch,
            only,
            held_out,
            ner,
            ocr,
            redact,
            json,
        } => {
            let inventory = veilq_eval::ablate::Inventory::read(&dir.join("patches.toml"))?;
            let ids: Vec<String> = if patch.is_empty() {
                inventory.patch.iter().map(|p| p.id.clone()).collect()
            } else {
                patch
            };
            let mut dirs = vec![dir.clone()];
            for seed in &held_out {
                let tmp =
                    std::env::temp_dir().join(format!("veilq-eval-{seed}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&tmp);
                generate::write(&tmp, *seed)?;
                dirs.push(tmp);
            }
            let effects = veilq_eval::ablate::run(&dirs, &ids, only.as_deref(), ner, ocr, redact);
            for d in &dirs[1..] {
                let _ = std::fs::remove_dir_all(d);
            }
            let effects = effects?;
            if !json {
                println!(
                    "{:<28} {:>8} {:>7} {:>6}  profiles (misses, fp)",
                    "patch off", "verdict", "misses", "fp"
                );
            }
            for e in &effects {
                if json {
                    println!("{}", serde_json::to_string(e)?);
                    continue;
                }
                let profiles: Vec<String> = e
                    .profiles
                    .iter()
                    .map(|(n, (m, f))| format!("{n} ({m:+}, {f:+})"))
                    .collect();
                println!(
                    "{:<28} {:>8} {:>+7} {:>+6}  {}",
                    e.id,
                    e.verdict(),
                    e.misses,
                    e.false_positives,
                    profiles.join(", ")
                );
            }
            Ok(true)
        }
        Command::Candidates {
            dir,
            only,
            ner,
            ocr,
            out,
        } => {
            let mut file = std::io::BufWriter::new(std::fs::File::create(&out)?);
            let w = candidates::write(&dir, only.as_deref(), ner, ocr, &mut file)?;
            std::io::Write::flush(&mut file)?;
            println!(
                "{} candidates ({} personal) from {} documents written to {}; {} skipped (containers, pictures without --ocr, unreadable truth)",
                w.candidates,
                w.personal,
                w.documents,
                out.display(),
                w.skipped
            );
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
            redact,
            jobs,
        } => {
            let gate_path = gate.unwrap_or_else(|| dir.join("gate.toml"));
            let gate: check::Gate = toml::from_str(&std::fs::read_to_string(&gate_path)?)
                .map_err(|e| anyhow::anyhow!("{}: {e}", gate_path.display()))?;
            let apart: std::collections::BTreeSet<String> = gate.profiles.keys().cloned().collect();
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
                        .map(|(_, d)| {
                            s.spawn(|| {
                                check::evaluate(d, only.as_deref(), ner, ocr, redact, &apart)
                            })
                        })
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
    // The last three columns (#175): the findings the learned decider
    // marked, and recall and precision were they dropped.
    println!(
        "{:<24} {:>6} {:>8} {:>8} {:>9} {:>5} {:>9} {:>7} {:>8} {:>9}",
        "profile",
        "files",
        "truth",
        "recall",
        "precision",
        "fp",
        "fp/file",
        "marked",
        "recall-d",
        "precision-d"
    );
    for (name, p) in &eval.per_profile {
        println!(
            "{name:<24} {:>6} {:>8} {:>8.3} {:>9.3} {:>5} {:>9.2} {:>7} {:>8.3} {:>9.3}",
            p.files,
            p.truth,
            p.recall(),
            p.precision(),
            p.false_positives,
            p.false_per_file(),
            p.marked,
            p.recall_if_dropped(),
            p.precision_if_dropped()
        );
    }
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
