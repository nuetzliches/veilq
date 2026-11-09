# ADR-0018: The mapping file is age-encrypted, and revealed by the CLI only

- **Status:** Accepted (card M2, points 4 to 7, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [ADR-0001](0001-pseudonyms-are-a-keyed-hash-in-the-shape-type-hex8.md), [ADR-0006](0006-four-actions-mask-by-default.md), [ADR-0016](0016-the-mcp-server-returns-redacted-content-and-reports-only.md), plan O-3

## Context

`pseudonymize` and `fake` keep a document readable and linkable; sometimes the
owner needs the originals back (a support case is solved, the customer must
be called). Whatever makes that possible holds every value in clear text and
is the most valuable file veilq writes.

## Decision

- `veilq redact --mapping <file>` writes, beside the output, what each
  `pseudonymize` and `fake` replacement stood for: JSON lines
  `{"replacement", "entity", "value"}`, sorted and unique, encrypted with age
  to the X25519 recipients of the policy's `[mapping] recipients`
  (`age1...`). Without recipients `--mapping` is an error before anything
  runs: there is no unencrypted mapping. The mapping is written before the
  output appears, both through temporary files.
- `veilq reveal <file> --mapping <file> --identity <file>` puts the values
  back, longest replacement first, in one pass. A replacement that stands for
  two values (a pseudonym collision) is an error, not a guess. Only the CLI
  reveals: not the MCP server (ADR-0016), not the UI.
- Plan O-3: the pseudonym key is the project's, from the environment
  variable the policy names, as in M0. The same value gives the same token in
  every run of a project; a mapping from any run reveals it.

## Alternatives considered

- **A passphrase (age scrypt):** a secret to type or store in CI; recipients
  let CI write mappings only the owner can read.
- **A plain mapping, "kept safe":** it ends up in a ticket next to the output.
- **A key per run or per mapping file:** tokens would not link across runs,
  which is what `pseudonymize` is for.

## Consequences

- Losing the identity loses the way back; that is the point.
- `fake` stand-ins are put back wherever the same text appears in the file;
  they are synthetic, so a collision with text that was there is unlikely but
  possible.

## Enforced by

- `crates/veilq-core/src/mapping.rs` (`encrypt`, `decrypt`, `reveal`), with
  tests; `crates/veilq-core/src/policy.rs` (`Mapping`, recipients checked).
- `crates/veilq/src/main.rs` (`redact`, `reveal`, `write_atomically`);
  `crates/veilq/tests/cli.rs` (the round trip, another key, no recipients).
