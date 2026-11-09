use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Parser, Subcommand, ValueEnum};
use veilq_core::{Action, Engine, Format, Outcome, Policy, PseudonymKey, Report, models};

#[cfg(feature = "mcp")]
mod mcp;
mod sarif;
#[cfg(feature = "ui")]
mod ui;

/// Find and redact personal data in text, documents, images and screen
/// recordings.
///
/// Exit codes: 0 nothing found, 1 personal data found, 2 error.
#[derive(Parser)]
#[command(name = "veilq", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Find personal data; replace nothing. Exit code 1 when something is
    /// found: a CI gate.
    Scan {
        /// A file, a directory (every file in it, `.git` left out) or `-` for
        /// standard input.
        path: PathBuf,
        #[command(flatten)]
        input: InputArgs,
        /// The report as JSON (schema/report.schema.json); for a directory,
        /// one line `{"path", "report"}` per file.
        #[arg(long, conflicts_with = "sarif")]
        json: bool,
        /// SARIF 2.1.0, for code scanning.
        #[arg(long)]
        sarif: bool,
    },
    /// Write the input with its personal data replaced. Exit code 1 when
    /// something was replaced.
    Redact {
        /// A file, or `-` for standard input.
        path: PathBuf,
        /// Where the output goes; `-` (the default) for standard output. A
        /// file appears only once the run succeeded.
        #[arg(short, long, default_value = "-")]
        output: PathBuf,
        #[command(flatten)]
        input: InputArgs,
        /// Overrides the policy's actions for every type.
        #[arg(long, value_enum)]
        action: Option<ActionArg>,
        /// Also write the report (JSON) to this file.
        #[arg(long)]
        report: Option<PathBuf>,
        /// Also write what each `pseudonymize` and `fake` replacement stood
        /// for, encrypted to the policy's `[mapping] recipients` (age).
        #[arg(long)]
        mapping: Option<PathBuf>,
    },
    /// Put the original values back into a redacted file, from its mapping.
    Reveal {
        /// A redacted file, or `-` for standard input.
        path: PathBuf,
        /// Where the output goes; `-` (the default) for standard output.
        #[arg(short, long, default_value = "-")]
        output: PathBuf,
        /// The mapping `veilq redact --mapping` wrote.
        #[arg(long)]
        mapping: PathBuf,
        /// An age identity file (`AGE-SECRET-KEY-...`) of one of the
        /// mapping's recipients.
        #[arg(long)]
        identity: PathBuf,
    },
    /// Run the MCP server: over stdio, or over Streamable HTTP with --http.
    ///
    /// Tools: scan, redact_text, redact_file, list_detectors. Over HTTP every
    /// request needs `Authorization: Bearer <token>`, the token from
    /// VEILQ_MCP_TOKEN or printed once at start.
    #[cfg(feature = "mcp")]
    Mcp {
        /// The directory files may be read from; by default the current
        /// directory.
        #[arg(long)]
        root: Option<PathBuf>,
        /// Serve Streamable HTTP at this address (`127.0.0.1:4770`) at
        /// `/mcp`, instead of stdio. Another address than loopback is
        /// reachable from the network: the token still guards it.
        #[arg(long)]
        http: Option<std::net::SocketAddr>,
        /// The policy file, found as for `policy check`.
        #[arg(long)]
        policy: Option<PathBuf>,
    },
    /// Work with the policy file (veilq.toml).
    Policy {
        #[command(subcommand)]
        command: PolicyCommand,
    },
    /// Open a local web UI that sets the input beside the redacted output.
    ///
    /// It listens on 127.0.0.1 only and prints a URL with a token; reads
    /// files under --root only.
    #[cfg(feature = "ui")]
    Ui {
        /// The directory the UI may read files from; by default the current
        /// directory.
        #[arg(long)]
        root: Option<PathBuf>,
        /// The port; by default a free one.
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// The address a reverse proxy on loopback serves this server under
        /// (`http://127.0.0.1:<port>`), as devstack does: requests that name
        /// it are accepted too, and the printed URL uses it.
        #[arg(long, value_parser = ui::parse_origin)]
        origin: Option<u16>,
        /// Print the URL only, do not open a browser.
        #[arg(long)]
        no_open: bool,
        /// The policy file, found as for `policy check`.
        #[arg(long)]
        policy: Option<PathBuf>,
    },
}

#[derive(clap::Args)]
struct InputArgs {
    /// The input's format; by default from the file's extension, `text` for
    /// standard input.
    #[arg(long, value_parser = ["text", "ndjson", "json", "csv", "tsv"])]
    format: Option<String>,
    /// The policy file; by default veilq.toml in the current directory, else
    /// in the git repository's root, else the built-in defaults.
    #[arg(long)]
    policy: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum ActionArg {
    Mask,
    Pseudonymize,
    Fake,
    Remove,
}

impl From<ActionArg> for Action {
    fn from(a: ActionArg) -> Self {
        match a {
            ActionArg::Mask => Action::Mask,
            ActionArg::Pseudonymize => Action::Pseudonymize,
            ActionArg::Fake => Action::Fake,
            ActionArg::Remove => Action::Remove,
        }
    }
}

#[derive(Subcommand)]
enum PolicyCommand {
    /// Check the policy a run here would use, and that the detectors it
    /// requires can run.
    Check {
        /// The policy file; by default veilq.toml in the current directory,
        /// else in the git repository's root, else the built-in defaults.
        #[arg(long)]
        policy: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let outcome = match run(cli) {
        Ok(outcome) => outcome,
        Err(e) => {
            eprintln!("error: {e:#}");
            Outcome::Error
        }
    };
    ExitCode::from(outcome.code())
}

fn run(cli: Cli) -> anyhow::Result<Outcome> {
    match cli.command {
        Command::Scan {
            path,
            input,
            json,
            sarif,
        } => scan(&path, &input, json, sarif),
        Command::Redact {
            path,
            output,
            input,
            action,
            report,
            mapping,
        } => redact(
            &path,
            &output,
            &input,
            action.map(Action::from),
            report.as_deref(),
            mapping.as_deref(),
        ),
        Command::Reveal {
            path,
            output,
            mapping,
            identity,
        } => reveal(&path, &output, &mapping, &identity),
        #[cfg(feature = "mcp")]
        Command::Mcp { root, http, policy } => {
            let (policy, _) = load_policy(policy.as_deref())?;
            let engine = engine(&policy, None)?;
            mcp::run(mcp::Options {
                root: root.unwrap_or(std::env::current_dir()?),
                http,
                policy,
                engine,
            })?;
            Ok(Outcome::Clean)
        }
        Command::Policy {
            command: PolicyCommand::Check { policy },
        } => {
            let (policy, source) = load_policy(policy.as_deref())?;
            veilq_core::detect::Detector::new(&policy)?;
            println!(
                "ok: {source} ({} entity rules, {} patterns, locales {}, default action {})",
                policy.entities.len(),
                policy.patterns.len(),
                policy.locales.join(", "),
                policy.defaults.action.name()
            );
            Ok(Outcome::Clean)
        }
        #[cfg(feature = "ui")]
        Command::Ui {
            root,
            port,
            origin,
            no_open,
            policy,
        } => {
            let (policy, source) = load_policy(policy.as_deref())?;
            let engine = engine(&policy, None)?;
            ui::run(ui::Options {
                root: root.unwrap_or(std::env::current_dir()?),
                port,
                proxy_port: origin,
                open: !no_open,
                policy,
                policy_source: source.to_string(),
                engine,
            })?;
            Ok(Outcome::Clean)
        }
    }
}

fn load_policy(explicit: Option<&Path>) -> anyhow::Result<(Policy, veilq_core::policy::Source)> {
    let cwd = std::env::current_dir()?;
    let (policy, source) = Policy::discover(explicit, &cwd)?;
    models::check_required(&policy.detectors.require, &models::model_dir())?;
    Ok((policy, source))
}

/// The engine of a run: the policy's key, if any, and a seed for `fake`
/// (from the key, else random for this run).
fn engine(policy: &Policy, action: Option<Action>) -> anyhow::Result<Engine> {
    let key = PseudonymKey::from_policy_env(policy)?;
    let seed = match &key {
        Some(k) => k.fake_seed(),
        None => {
            let mut seed = vec![0u8; 32];
            getrandom::fill(&mut seed).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
            seed
        }
    };
    Ok(Engine::new(policy, key, &seed, action)?)
}

fn format_of(path: &Path, explicit: Option<&str>) -> anyhow::Result<Format> {
    Ok(match explicit {
        Some(name) => Format::from_name(name)?,
        None if is_stdin(path) => Format::Text,
        None => Format::from_path(path)?,
    })
}

fn is_stdin(path: &Path) -> bool {
    path.as_os_str() == "-"
}

fn open(path: &Path) -> anyhow::Result<Box<dyn std::io::BufRead>> {
    Ok(if is_stdin(path) {
        Box::new(BufReader::new(std::io::stdin().lock()))
    } else {
        let file = std::fs::File::open(path).with_context(|| path.display().to_string())?;
        Box::new(BufReader::new(file))
    })
}

/// Every file under `dir`, sorted, `.git` left out.
fn files_under(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| dir.display().to_string())?
        .collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        if e.file_type()?.is_dir() {
            if e.file_name() != ".git" {
                files_under(&path, out)?;
            }
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn scan(path: &Path, input: &InputArgs, json: bool, sarif: bool) -> anyhow::Result<Outcome> {
    let (policy, _) = load_policy(input.policy.as_deref())?;
    let engine = engine(&policy, None)?;
    let dir = !is_stdin(path) && path.is_dir();
    let files = if dir {
        let mut files = Vec::new();
        files_under(path, &mut files)?;
        files
    } else {
        vec![path.to_path_buf()]
    };
    // Every format first: a file veilq cannot read fails the run before any
    // result is printed (ADR-0003).
    let formats = files
        .iter()
        .map(|f| format_of(f, input.format.as_deref()))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut results: Vec<(String, Report)> = Vec::new();
    for (file, format) in files.iter().zip(formats) {
        let run = engine
            .run(open(file)?, None, format)
            .with_context(|| file.display().to_string())?;
        let name = if dir {
            display_relative(path, file)
        } else {
            file.display().to_string()
        };
        results.push((name, run.report));
    }
    let total: usize = results.iter().map(|(_, r)| r.findings.len()).sum();
    let stdout = &mut std::io::stdout().lock();
    if sarif {
        serde_json::to_writer_pretty(&mut *stdout, &sarif::sarif(&results))?;
        writeln!(stdout)?;
    } else if json && !dir {
        serde_json::to_writer_pretty(&mut *stdout, &results[0].1)?;
        writeln!(stdout)?;
    } else if json {
        for (path, report) in &results {
            serde_json::to_writer(
                &mut *stdout,
                &serde_json::json!({ "path": path, "report": report }),
            )?;
            writeln!(stdout)?;
        }
    } else {
        for (path, report) in &results {
            for f in &report.findings {
                writeln!(
                    stdout,
                    "{path}:{}: {} ({}, {:.2})",
                    sarif::describe(&f.location),
                    f.entity.as_str(),
                    f.detector,
                    f.confidence
                )?;
            }
        }
        eprintln!("{total} finding(s) in {} file(s)", results.len());
    }
    Ok(if total > 0 {
        Outcome::Findings
    } else {
        Outcome::Clean
    })
}

fn reveal(path: &Path, output: &Path, mapping: &Path, identity: &Path) -> anyhow::Result<Outcome> {
    let data = std::fs::read(mapping).with_context(|| mapping.display().to_string())?;
    let identity_text =
        std::fs::read_to_string(identity).with_context(|| identity.display().to_string())?;
    let entries = veilq_core::mapping::decrypt(&data, &identity_text)?;
    let mut text = String::new();
    open(path)?
        .read_to_string(&mut text)
        .with_context(|| path.display().to_string())?;
    let (revealed, n) = veilq_core::mapping::reveal(&text, &entries)?;
    if is_stdin(output) {
        std::io::stdout().lock().write_all(revealed.as_bytes())?;
    } else {
        write_atomically(output, |w| Ok(w.write_all(revealed.as_bytes())?))?;
    }
    eprintln!("{n} replacement(s) put back");
    Ok(Outcome::Clean)
}

fn display_relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Writes a file through a temporary one beside it, renamed once `write`
/// succeeded: on an error nothing is left that could be mistaken for a result.
fn write_atomically<T>(
    target: &Path,
    write: impl FnOnce(&mut dyn Write) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let tmp = target.with_file_name(format!(
        ".{}.veilq-tmp",
        target
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default()
    ));
    let result = (|| {
        let mut file = std::io::BufWriter::new(
            std::fs::File::create(&tmp).with_context(|| tmp.display().to_string())?,
        );
        let value = write(&mut file)?;
        file.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        Ok::<_, anyhow::Error>(value)
    })();
    match result {
        Ok(value) => {
            std::fs::rename(&tmp, target).with_context(|| target.display().to_string())?;
            Ok(value)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn redact(
    path: &Path,
    output: &Path,
    input: &InputArgs,
    action: Option<Action>,
    report_path: Option<&Path>,
    mapping_path: Option<&Path>,
) -> anyhow::Result<Outcome> {
    if !is_stdin(path) && path.is_dir() {
        anyhow::bail!("{}: redact takes one file", path.display());
    }
    let (policy, _) = load_policy(input.policy.as_deref())?;
    // Before anything runs: a mapping needs someone to encrypt it to.
    let recipients = match mapping_path {
        Some(_) => Some(
            policy
                .mapping
                .as_ref()
                .map(|m| m.recipients.clone())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "--mapping needs `[mapping] recipients` in the policy: a mapping is never written unencrypted"
                    )
                })?,
        ),
        None => None,
    };
    let engine = engine(&policy, action)?;
    let format = format_of(path, input.format.as_deref())?;
    let reader = open(path)?;
    let run = if is_stdin(output) {
        let mut stdout = std::io::stdout().lock();
        engine.run(reader, Some(&mut stdout), format)?
    } else {
        // The mapping is encrypted before the output appears, so that a
        // pseudonymized output never exists without its way back.
        write_atomically(output, |w| {
            let run = engine.run(reader, Some(w), format)?;
            if let (Some(p), Some(r)) = (mapping_path, &recipients) {
                let data = veilq_core::mapping::encrypt(&run.mapping, r)?;
                write_atomically(p, |m| Ok(m.write_all(&data)?))?;
            }
            Ok(run)
        })?
    };
    if is_stdin(output)
        && let (Some(p), Some(r)) = (mapping_path, &recipients)
    {
        let data = veilq_core::mapping::encrypt(&run.mapping, r)?;
        write_atomically(p, |m| Ok(m.write_all(&data)?))?;
    }
    if let Some(p) = report_path {
        let text = serde_json::to_string_pretty(&run.report)? + "\n";
        std::fs::write(p, text).with_context(|| p.display().to_string())?;
    }
    let n = run.report.findings.len();
    eprintln!("{n} finding(s) replaced");
    Ok(if n > 0 {
        Outcome::Findings
    } else {
        Outcome::Clean
    })
}
