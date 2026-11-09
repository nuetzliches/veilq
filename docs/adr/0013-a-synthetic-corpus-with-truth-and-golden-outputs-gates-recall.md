# ADR-0013: A synthetic corpus with truth and golden outputs gates recall

- **Status:** Accepted (card M1, point 5, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [plan.md §6](../plan.md#6-evaluation), R10

## Context

Detection quality has to be measured, per type and per format, and a change
must show what it does to real outputs. Real personal data never goes into
the repository, so the measurement needs data that is synthetic and still
knows exactly where its personal data is.

## Decision

`crates/veilq-eval` generates documents from a seed:
`eval/<locale>/<format>/` (today `de/{text,csv,ndjson,json}`,
`en/{text,csv}`), each with a `<file>.truth.json` that lists every piece of
personal data in the shape of a finding (type and location, no detector, no
confidence). Documents hold look-alikes that are not personal data, and names
without a marking word (for NER, M3). The corpus of seed 1 is committed.

`veilq-eval check eval` measures **recall** (truth no byte of which is left in
the output, per type) and **precision**; `eval/gate.toml` sets the minimum.
CI runs it on the corpus and on five seeds generated in CI only
(`--held-out`). Golden tests redact every committed document with the
built-in defaults and compare output and report with `eval/golden/`.

## Alternatives considered

- **Hand-written fixtures:** few, and the person who writes the detector
  writes the cases it passes.
- **Snapshot tests with `insta`:** one more dependency for what the schema
  test already does with an environment variable.
- **Exact span matching:** a finding a few bytes wider than the truth (a
  street with its city) would count as a miss though nothing leaks.

## Consequences

- The corpus and the detectors are written by the same side: a recall of 1.0
  says the rules cover the templates, not the world. Held-out seeds help
  against overfitting to values, not to templates; new templates come from
  real misses, as synthetic reproductions.
- A change to an output is a diff in `eval/golden/`.

## Enforced by

- `crates/veilq-eval/src/{generate,check}.rs`, `crates/veilq-eval/tests/golden.rs`.
- `eval/gate.toml`; CI job `eval` (`.github/workflows/ci.yml`).
