use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use veilq_core::{Outcome, Policy, models};

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
    }
}
