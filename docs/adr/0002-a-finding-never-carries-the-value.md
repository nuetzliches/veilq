# ADR-0002: A finding never carries the value it found

- **Status:** Accepted (card M0, point 2, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [plan.md §4.3](../plan.md#43-schema)

## Context

Reports go where the content must not: CI logs, SARIF uploads, an agent's
context, a ticket. A report that quotes what it found is a copy of the
personal data, in a place nobody redacts.

## Decision

A finding names the entity type, the location, the confidence and the
detector. Its only value-derived field is the keyed pseudonym (ADR-0001), and
only when the action is `pseudonymize`. There is no field for the value and
none for an unkeyed hash of it; readers reject unknown fields. The schema is
committed in `schema/report.schema.json`, versioned (`version: 1`).

## Alternatives considered

- **The value, for debugging:** the report becomes the leak.
- **An unkeyed hash, to compare runs:** reversible for small value spaces.
- **A context excerpt around the finding:** carries the neighbouring personal
  data too.

## Consequences

- To see what was found, a person looks at the input and the location, on the
  machine the input is on.
- Bug reports about misses need a synthetic reproduction (SECURITY.md).

## Enforced by

- `crates/veilq-core/src/finding.rs` (`Finding`, `Location`, `Report`, `deny_unknown_fields`), with a test that a `value` field is rejected.
- `schema/report.schema.json` and `crates/veilq-core/tests/schema.rs`: a type change shows up as a schema diff.
