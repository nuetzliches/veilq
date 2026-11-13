use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Parser, Subcommand, ValueEnum};
use veilq_core::{Action, Engine, Format, Outcome, Policy, PseudonymKey, Report, models};

#[cfg(feature = "ui")]
mod desktop;
mod explain;
mod fetch;
#[cfg(feature = "mcp")]
mod mcp;
mod package;
mod sarif;
#[cfg(feature = "ui")]
mod ui;
mod update;

/// Find and redact personal data in text, documents, images and screen
/// recordings.
///
/// Exit codes: 0 nothing found, 1 personal data found, 2 error.
///
/// Started by a double click, veilq opens its web UI in a window (ADR-0027).
#[derive(Parser)]
#[command(name = "veilq", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
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
        /// Also say what veilq read and why it decided as it did: per text it
        /// read (a line, a cell, a picture's OCR line) every candidate with
        /// its detector, confidence, threshold and verdict. On standard
        /// error, beside the report; with `--json` one JSON line per text
        /// (unstable). It holds the values in clear: for a look on your own
        /// machine, not for a log or a ticket.
        #[arg(long, conflicts_with = "sarif")]
        explain: bool,
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
        /// A picture whose verify rounds do not settle (each finds something
        /// new) is written anyway, with the last round's findings filled
        /// too; the report asks for a review (ADR-0026). Without it, that is
        /// an error.
        #[arg(long)]
        unverified: bool,
    },
    /// Decide on a ticket's body (Markdown) for a gateway: hand it on as it
    /// is when its first line is a matching `veilq: cleaned|pass <hash>` or
    /// an exempt prefix, else redact it and mark it (ADR-0024).
    ///
    /// Prints JSON: `state` (`kept`, `exempt`, `cleaned`, `pass`), `text` (what
    /// the agent may read), `write` (the body to write back, when it
    /// changed) and the `report`. Exit code 1 when something was replaced.
    Mark {
        /// The body, or `-` for standard input.
        path: PathBuf,
        /// The policy file; by default veilq.toml in the current directory,
        /// else in the git repository's root, else the built-in defaults.
        #[arg(long)]
        policy: Option<PathBuf>,
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
    /// Models the detectors need (ADR-0005, ADR-0022).
    Models {
        #[command(subcommand)]
        command: ModelsCommand,
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
        /// Keep the token in this file, so that it survives a restart: read
        /// it if the file exists, else write a new one there. Without it, a
        /// new token per start.
        #[arg(long)]
        token_file: Option<PathBuf>,
        /// Print the URL only, do not open a browser.
        #[arg(long)]
        no_open: bool,
        /// The policy file, found as for `policy check`.
        #[arg(long)]
        policy: Option<PathBuf>,
    },
    /// Check for a newer release, and install it where this program is
    /// (only when asked; nothing but the request is sent, ADR-0027).
    Update {
        /// Only say whether there is a newer release.
        #[arg(long)]
        check: bool,
    },
    /// What a double click starts, without a console window: veilq starts
    /// itself so where Windows gave the click one (ADR-0031).
    #[cfg(feature = "ui")]
    #[command(hide = true)]
    Desktop,
    /// Remove what setup installed for this user: shortcuts, the PATH
    /// entry, the entry in "Installed apps" and the program (ADR-0027).
    #[cfg(feature = "ui")]
    Uninstall {
        /// Remove the downloaded models too.
        #[arg(long)]
        models: bool,
    },
}

#[derive(clap::Args)]
struct InputArgs {
    /// The input's format; by default from the file's extension, `text` for
    /// standard input.
    #[arg(long, value_parser = ["text", "markdown", "ndjson", "json", "csv", "tsv", "docx", "xlsx", "pptx", "eml", "png", "jpeg", "webp", "gif", "pdf"])]
    format: Option<String>,
    /// The policy file; by default veilq.toml in the current directory, else
    /// in the git repository's root, else the built-in defaults.
    #[arg(long)]
    policy: Option<PathBuf>,
    /// What this run is for, instead of the policy's purposes (ADR-0040):
    /// `legal`, `development`; `general` is always one. Repeat it for more
    /// than one.
    #[arg(long = "purpose", value_parser = clap::builder::PossibleValuesParser::new(veilq_core::purpose::PURPOSES))]
    purposes: Vec<String>,
}

impl InputArgs {
    /// The policy of the run: the one found, its purposes replaced by
    /// `--purpose` when given.
    fn policy(&self) -> anyhow::Result<Policy> {
        let (mut policy, _) = load_policy(self.policy.as_deref())?;
        if !self.purposes.is_empty() {
            policy.purposes = self.purposes.clone();
        }
        Ok(policy)
    }
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
enum ModelsCommand {
    /// What is pinned for this platform and what is installed.
    List,
    /// Download a detector's files from their pinned URLs (with `curl`) and
    /// install what matches its SHA-256.
    Fetch {
        /// `ner`, `ocr` or `pdf`.
        detector: String,
    },
    /// Install a detector's files from a directory that holds the downloads
    /// (by their file names), for a machine without network.
    Install {
        detector: String,
        #[arg(long)]
        from: PathBuf,
    },
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
    // In an MSIX package the models live in the package's folder (ADR-0039).
    if let Some(dir) = package::local_data() {
        models::set_local_data(dir);
    }
    update::clean_up();
    let Some(command) = cli.command else {
        #[cfg(feature = "ui")]
        if desktop::double_clicked() {
            if desktop::handed_on() {
                return Ok(Outcome::Clean);
            }
            return start_desktop();
        }
        use clap::CommandFactory;
        Cli::command().print_help()?;
        return Ok(Outcome::Error);
    };
    match command {
        Command::Scan {
            path,
            input,
            json,
            sarif,
            explain,
        } => scan(&path, &input, json, sarif, explain),
        Command::Redact {
            path,
            output,
            input,
            action,
            report,
            mapping,
            unverified,
        } => redact(
            &path,
            &output,
            &input,
            action.map(Action::from),
            report.as_deref(),
            mapping.as_deref(),
            unverified,
        ),
        Command::Reveal {
            path,
            output,
            mapping,
            identity,
        } => reveal(&path, &output, &mapping, &identity),
        Command::Mark { path, policy } => {
            let (policy, _) = load_policy(policy.as_deref())?;
            let engine = engine(&policy, None)?;
            let mut body = String::new();
            open(&path)?
                .read_to_string(&mut body)
                .context("the body is not UTF-8 text")?;
            let marked = veilq_core::marker::mark(&engine, &body)?;
            println!("{}", serde_json::to_string(&marked)?);
            Ok(if marked.state == veilq_core::marker::State::Cleaned {
                Outcome::Findings
            } else {
                Outcome::Clean
            })
        }
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
        Command::Models { command } => models_command(command),
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
            token_file,
            no_open,
            policy,
        } => {
            let (policy, source) = load_policy(policy.as_deref())?;
            // The server answers at once; the models load beside it.
            let engine = patterns_engine(&policy)?;
            let full = policy.clone();
            ui::run(ui::Options {
                root: root.unwrap_or(std::env::current_dir()?),
                port,
                proxy_port: origin,
                token_file,
                open: !no_open,
                policy,
                policy_source: source.to_string(),
                engine,
                load: Some(Box::new(move || crate::engine(&full, None))),
                desktop: false,
                reload: Some(Box::new(|p: &Policy| crate::engine(p, None))),
                window: None,
            })?;
            Ok(Outcome::Clean)
        }
        Command::Update { check } => {
            package::refuse("updates")?;
            let found = update::check()?;
            if !found.newer {
                println!("veilq {} is the newest release", found.current);
                return Ok(Outcome::Clean);
            }
            println!(
                "veilq {} is out (this is {}): {}",
                found.latest, found.current, found.url
            );
            if !check {
                let (_, program) = update::install_latest()?;
                println!("installed: {}", program.display());
            }
            Ok(Outcome::Clean)
        }
        #[cfg(feature = "ui")]
        Command::Desktop => start_desktop(),
        #[cfg(feature = "ui")]
        Command::Uninstall { models } => {
            desktop::uninstall(&desktop::Uninstall { models })?;
            println!("veilq is removed for this user; its folder goes once this window closes.");
            Ok(Outcome::Clean)
        }
    }
}

/// A double click: bring the running UI to the front, or start one that
/// reads the user's home folder (ADR-0027), in veilq's own window where
/// WebView2 is there (ADR-0029), else in a browser's app window. An error
/// stays on screen until a key is pressed, since the console closes with
/// the program.
#[cfg(feature = "ui")]
fn start_desktop() -> anyhow::Result<Outcome> {
    if let Some(instance) = desktop::running_instance() {
        if !desktop::focus_running(&instance) {
            desktop::open_app_window(&instance.url);
        }
        return Ok(Outcome::Clean);
    }
    let result = (|| -> anyhow::Result<()> {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .map_or_else(std::env::current_dir, Ok)?;
        // The folder chosen in Settings, while it is there.
        let home = desktop::Settings::load()
            .root
            .filter(|r| r.is_dir())
            .unwrap_or(home);
        let (policy, source) = load_policy(None)?;
        // The window shows at once; the models load beside it (ADR-0032).
        let engine = patterns_engine(&policy)?;
        let full = policy.clone();
        let options = ui::Options {
            root: home,
            port: 0,
            proxy_port: None,
            token_file: Some(desktop::app_dir().join("ui-token")),
            open: false,
            policy,
            policy_source: source.to_string(),
            engine,
            load: Some(Box::new(move || crate::engine(&full, None))),
            desktop: true,
            reload: Some(Box::new(|p: &Policy| crate::engine(p, None))),
            window: None,
        };
        #[cfg(windows)]
        if desktop::window::available() {
            return desktop::window::run(options);
        }
        ui::run(options)
    })();
    if let Err(e) = result {
        desktop::report_error(&format!("veilq could not start:\n\n{e:#}"));
        return Ok(Outcome::Error);
    }
    Ok(Outcome::Clean)
}

fn load_policy(explicit: Option<&Path>) -> anyhow::Result<(Policy, veilq_core::policy::Source)> {
    let cwd = std::env::current_dir()?;
    let (policy, source) = Policy::discover(explicit, &cwd)?;
    models::check_required(&policy.detectors.require, &models::model_dir())?;
    Ok((policy, source))
}

/// The engine of a run: the policy's key, if any, and a seed for `fake`
/// (from the key, else random for this run).
/// The engine with its patterns only: quick to make, for a UI that shows
/// at once while [`engine`] loads the models (ADR-0032).
#[cfg(feature = "ui")]
fn patterns_engine(policy: &Policy) -> anyhow::Result<Engine> {
    let key = PseudonymKey::from_policy_env(policy)?;
    let seed = fake_seed(&key)?;
    Ok(Engine::new(policy, key, &seed, None)?)
}

fn fake_seed(key: &Option<PseudonymKey>) -> anyhow::Result<Vec<u8>> {
    Ok(match key {
        Some(k) => k.fake_seed(),
        None => {
            let mut seed = vec![0u8; 32];
            getrandom::fill(&mut seed).map_err(|e| anyhow::anyhow!("no randomness: {e}"))?;
            seed
        }
    })
}

fn engine(policy: &Policy, action: Option<Action>) -> anyhow::Result<Engine> {
    let key = PseudonymKey::from_policy_env(policy)?;
    let seed = fake_seed(&key)?;
    #[allow(unused_mut)]
    let mut engine = Engine::new(policy, key, &seed, action)?;
    let dir = models::model_dir();
    #[cfg(feature = "ner")]
    if let Some((model, tokenizer, runtime)) = models::ner_files(&dir) {
        let ner = veilq_core::ner::Ner::load(&veilq_core::ner::ModelFiles {
            model: &model,
            tokenizer: &tokenizer,
            runtime: &runtime,
        })?;
        engine = engine.with_ner(std::sync::Arc::new(ner));
    }
    #[cfg(feature = "ocr")]
    if let Some((det, rec, dict, runtime)) = models::ocr_files(&dir) {
        let ocr = veilq_core::ocr::Ocr::load(&veilq_core::ocr::ModelFiles {
            det: &det,
            rec: &rec,
            dict: &dict,
            runtime: &runtime,
        })?;
        engine = engine.with_ocr(std::sync::Arc::new(ocr));
    }
    #[cfg(feature = "ocr")]
    if let Some((model, runtime)) = models::face_files(&dir) {
        let faces = veilq_core::face::Faces::load(&veilq_core::face::ModelFiles {
            model: &model,
            runtime: &runtime,
        })?;
        engine = engine.with_faces(std::sync::Arc::new(faces));
    }
    #[cfg(feature = "pdf")]
    if let Some(library) = models::pdf_library(&dir) {
        engine = engine.with_pdfium(veilq_core::pdf::pdfium(&library)?);
    }
    let _ = &dir;
    Ok(engine)
}

fn models_command(command: ModelsCommand) -> anyhow::Result<Outcome> {
    let dir = models::model_dir();
    match command {
        ModelsCommand::List => {
            println!("model directory: {}", dir.display());
            for d in models::COMPONENTS {
                let files = models::pinned_for(d);
                if files.is_empty() {
                    continue;
                }
                let state = if models::installed(d, &dir) {
                    "installed"
                } else {
                    "not installed"
                };
                println!("{d}: {state}");
                for f in files {
                    let here = if dir.join(f.file).is_file() {
                        "ok"
                    } else {
                        "missing"
                    };
                    println!("  {here:7} {}  <- {}", f.file, f.url());
                }
            }
            Ok(Outcome::Clean)
        }
        ModelsCommand::Fetch { detector } => {
            pinned_files(&detector)?;
            let shown = std::sync::Mutex::new(String::new());
            fetch::component(&detector, &dir, &|p| {
                if let Ok(mut s) = shown.lock()
                    && *s != p.file
                {
                    println!("fetching {} ({}/{})", p.file, p.index, p.of);
                    s.clone_from(&p.file);
                }
            })?;
            println!("ok       {detector} in {}", dir.display());
            Ok(Outcome::Clean)
        }
        ModelsCommand::Install { detector, from } => {
            for f in pinned_files(&detector)? {
                if dir.join(f.file).is_file() {
                    println!("ok       {}", f.file);
                    continue;
                }
                let src = from.join(f.name);
                if !src.is_file() {
                    anyhow::bail!("{} is not in {}", f.name, from.display());
                }
                println!("ok       {}", models::install(f, &src, &dir)?.display());
            }
            Ok(Outcome::Clean)
        }
    }
}

fn pinned_files(detector: &str) -> anyhow::Result<Vec<&'static models::PinnedFile>> {
    let files = models::pinned_for(detector);
    if files.is_empty() {
        anyhow::bail!("this release pins no files for `{detector}` on this platform");
    }
    Ok(files)
}

fn format_of(path: &Path, explicit: Option<&str>) -> anyhow::Result<Format> {
    Ok(match explicit {
        Some(name) => Format::from_name(name)?,
        None if is_stdin(path) => Format::Text,
        None => Format::from_path(path)?,
    })
}

/// A run of any format: text formats streamed, containers read whole.
fn run_any(
    engine: &Engine,
    mut reader: Box<dyn std::io::BufRead>,
    output: Option<&mut dyn Write>,
    format: Format,
) -> anyhow::Result<veilq_core::engine::Run> {
    if !format.is_container() {
        return Ok(engine.run(reader, output, format)?);
    }
    let mut input = Vec::new();
    reader.read_to_end(&mut input)?;
    let (bytes, run) = engine.run_bytes(&input, format, output.is_some())?;
    if let Some(w) = output {
        w.write_all(&bytes)?;
    }
    Ok(run)
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

fn scan(
    path: &Path,
    input: &InputArgs,
    json: bool,
    sarif: bool,
    explain: bool,
) -> anyhow::Result<Outcome> {
    let policy = input.policy()?;
    let explanation = veilq_core::Explain::new();
    let mut engine = engine(&policy, None)?;
    if explain {
        engine = engine.with_explain(explanation.clone());
    }
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
    let mut readings = Vec::new();
    for (file, format) in files.iter().zip(formats) {
        let run = run_any(&engine, open(file)?, None, format)
            .with_context(|| file.display().to_string())?;
        let name = if dir {
            display_relative(path, file)
        } else {
            file.display().to_string()
        };
        if explain {
            readings.push((name.clone(), explanation.take()));
        }
        results.push((name, run.report));
    }
    if explain {
        explain::write(&mut std::io::stderr().lock(), &readings, json)?;
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
    unverified: bool,
) -> anyhow::Result<Outcome> {
    if !is_stdin(path) && path.is_dir() {
        anyhow::bail!("{}: redact takes one file", path.display());
    }
    let policy = input.policy()?;
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
    let engine = engine(&policy, action)?.with_unverified(unverified);
    let format = format_of(path, input.format.as_deref())?;
    let reader = open(path)?;
    let run = if is_stdin(output) {
        let mut stdout = std::io::stdout().lock();
        run_any(&engine, reader, Some(&mut stdout), format)?
    } else {
        // The mapping is encrypted before the output appears, so that a
        // pseudonymized output never exists without its way back.
        write_atomically(output, |w| {
            let run = run_any(&engine, reader, Some(w), format)?;
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
