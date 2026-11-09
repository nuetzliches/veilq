# ADR-0006: Four actions, `mask` by default

- **Status:** Accepted (card M0, point 6, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [plan.md §3.3](../plan.md#33-actions)

## Context

What replaces a finding decides what a reader can still learn: the type
only, that two places are the same value, a realistic stand-in, or nothing.

## Decision

`mask` writes `<TYPE>`. `pseudonymize` writes `[TYPE:hex8]` (ADR-0001) and
needs a key. `fake` writes a plausible value of the same type and shape
(built in M1: `crates/veilq-core/src/fake.rs`). `remove` drops the span. The
default, per policy and per entity type, is `mask`.

## Alternatives considered

- **`pseudonymize` by default:** a run without a key would fail, and tokens
  link records, which the user did not ask for.
- **`remove` by default:** the reader no longer sees that something was there.

## Consequences

- A policy that wants linkage says so, and names a key.

## Enforced by

- `crates/veilq-core/src/action.rs` (`Action`, `Action::replacement`), with tests.
- `crates/veilq-core/src/policy.rs` (`Defaults`, `Policy::action_for`).
