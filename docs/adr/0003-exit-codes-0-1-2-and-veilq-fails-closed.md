# ADR-0003: Exit codes 0, 1, 2, and veilq fails closed

- **Status:** Accepted (card M0, point 3, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [plan.md §2 R4, §4.1](../plan.md#41-cli)

## Context

CI gates read the exit code only. A redaction tool that skips what it cannot
read and exits 0 makes the gate pass on exactly the content nobody checked.

## Decision

`0`: nothing above the threshold. `1`: personal data found above the
threshold. `2`: the run could not be completed: an unsupported format, an
unreadable part, an invalid policy, a required detector without its model,
an action that is not built, a command line error. On `2`, `redact` writes no
output that could be mistaken for a redacted one.

## Alternatives considered

- **Skip the unknown, warn, exit 0:** the gate passes unchecked content.
- **One code for "found" and "failed":** a gate cannot tell personal data
  from a broken run, and people learn to ignore it.

## Consequences

- A new format in a scanned tree turns a gate red until veilq reads it or the
  path is excluded on purpose.

## Enforced by

- `crates/veilq-core/src/outcome.rs` (`Outcome`), `crates/veilq/src/main.rs` (every error ends as `Outcome::Error`; clap exits 2 on argument errors).
- `crates/veilq-core/src/engine.rs`: an unknown format, broken JSON or CSV, input that is not UTF-8, and personal data left in an output (verify) are errors.
