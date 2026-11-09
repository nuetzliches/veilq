# ADR-0015: `scan` and `redact`: what they print and how they end

- **Status:** Proposed
- **Date:** 2026-11-09
- **Deciders:** agent, for the owner's review
- **Related:** [ADR-0003](0003-exit-codes-0-1-2-and-veilq-fails-closed.md), [plan.md §4.1](../plan.md#41-cli)

## Context

Plan §4.1 names the commands; M1 had to settle what they print, since scripts
and CI read it. These points were decided while building M1 and were not on
its card.

## Decision

- `scan <file|dir|->`: one line per finding, `path:line:column: TYPE
  (detector, confidence)`; `--json` prints the report of one input, or for a
  directory one line `{"path", "report"}` per file; `--sarif` prints SARIF
  2.1.0 (a rule per type, no snippet). A directory is scanned whole, `.git`
  left out; every file's format is known before anything is printed, or the
  run fails.
- `redact <file|-> -o <file|->`: the output, written to a temporary file
  beside the target and renamed on success, so exit 2 leaves nothing behind.
  To standard output it streams; on exit 2 what came before is redacted but
  incomplete. `--report` writes the report, `--action` overrides the policy.
- Exit codes as ADR-0003: `redact` exits 1 when it replaced something.
- The format comes from the extension (`.txt .log .md`, `.ndjson .jsonl`,
  `.json`, `.csv`, `.tsv`) or `--format`; anything else is an error, standard
  input is text.

## Alternatives considered

- **`redact` exits 0 on success:** easier in scripts, but a gate could no
  longer tell "nothing found" from "something replaced".
- **Unknown extensions as text:** an XML or HTML file would pass with entities
  (`&#64;`) veilq does not decode.

## Consequences

- `veilq scan .` in a repository fails on the first file veilq cannot read;
  scan the folders that hold data.

## Enforced by

- `crates/veilq/src/main.rs`, `crates/veilq/src/sarif.rs`,
  `crates/veilq-core/src/engine.rs` (`Format`).
