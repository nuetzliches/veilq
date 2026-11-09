# ADR-0005: Models are fetched explicitly and pinned in the binary

- **Status:** Accepted (card M0, point 5, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [plan.md §3.2, R8](../plan.md#32-detectors)

## Context

NER and OCR need models of tens to hundreds of megabytes. Whoever controls a
model controls what veilq finds: a swapped model that misses names is a
silent hole in every redaction.

## Decision

A model is downloaded only by `veilq models fetch`, from a URL with a SHA-256
compiled into the binary; a download that does not match is discarded. A run
never downloads. A detector the policy requires (`[detectors] require`)
whose model is not in the model directory (`VEILQ_MODEL_DIR`, else the
user's cache directory) ends the run with exit code 2.

## Alternatives considered

- **Download on first use:** a network call in the middle of a CI run, and a
  URL that can change under the binary.
- **Models inside the binary:** hundreds of megabytes for users who need
  patterns only.

## Consequences

- CI caches the model directory and runs `veilq models fetch` once.
- A new model is a new release.
- Until a model is pinned (plan O-1, O-2), a policy that requires `ner` or
  `ocr` fails, saying that this release has none.

## Enforced by

- `crates/veilq-core/src/models.rs` (`PINNED`, `check_required`, `model_dir`), with a test; `veilq policy check` runs the check.
- `veilq models fetch` itself comes with the first pinned model (M3).
