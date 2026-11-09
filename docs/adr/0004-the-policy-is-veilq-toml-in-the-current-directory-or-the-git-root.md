# ADR-0004: The policy is `veilq.toml`, in the current directory or the git root

- **Status:** Accepted (card M0, point 4, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [plan.md §4.4](../plan.md#44-policy-file)

## Context

A project keeps its own patterns, allow lists and actions in its repository.
Which file applies to a run must be obvious to someone reading the
repository, and a typo must not weaken it silently.

## Decision

`--policy <file>` names the policy (it must exist). Without it, veilq takes
`veilq.toml` from the current directory, else from the root of the git
repository the current directory is in (the nearest `.git` directory or
file), else its built-in defaults. Nothing in between and nothing above the
git root is read. The file is TOML with `version = 1`; unknown keys, unknown
detectors, invalid patterns, confidences outside 0 to 1 and `pseudonymize`
without `[pseudonymize] key_env` are errors (ADR-0003). The key itself never
goes into the file. The schema is `schema/policy.schema.json`.

## Alternatives considered

- **Search every directory upward** (as `.editorconfig`): a file in a parent
  directory outside the repository changes a run without anyone seeing it.
- **Ignore unknown keys:** `min_confidnce = 0.9` would leave the default in
  force, unnoticed.

## Consequences

- A monorepo with several policies passes `--policy` per part.

## Enforced by

- `crates/veilq-core/src/policy.rs` (`Policy::discover`, `Policy::validate`), with tests.
- `veilq policy check` in `crates/veilq/src/main.rs`.
- `schema/policy.schema.json` and `crates/veilq-core/tests/schema.rs`.
