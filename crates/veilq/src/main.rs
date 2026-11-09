use clap::Parser;

/// Find and redact personal data in text, documents, images and screen
/// recordings.
///
/// The commands (`scan`, `redact`, `mcp`) follow once the review card of M0
/// fixed their contract (docs/plan.md §4).
#[derive(Parser)]
#[command(name = "veilq", version)]
struct Cli {}

fn main() -> anyhow::Result<()> {
    let _cli = Cli::parse();
    Ok(())
}
