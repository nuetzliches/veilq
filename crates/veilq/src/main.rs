use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use veilq_core::{Outcome, Policy, models};

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
        Command::Policy {
            command: PolicyCommand::Check { policy },
        } => {
            let cwd = std::env::current_dir()?;
            let (policy, source) = Policy::discover(policy.as_deref(), &cwd)?;
            models::check_required(&policy.detectors.require, &models::model_dir())?;
            println!(
                "ok: {source} ({} entity rules, {} patterns, default action {})",
                policy.entities.len(),
                policy.patterns.len(),
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
            let cwd = std::env::current_dir()?;
            let (policy, source) = Policy::discover(policy.as_deref(), &cwd)?;
            models::check_required(&policy.detectors.require, &models::model_dir())?;
            ui::run(ui::Options {
                root: root.unwrap_or(cwd),
                port,
                proxy_port: origin,
                open: !no_open,
                policy,
                policy_source: source.to_string(),
            })?;
            Ok(Outcome::Clean)
        }
    }
}
