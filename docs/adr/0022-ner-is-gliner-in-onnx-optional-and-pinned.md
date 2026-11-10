# ADR-0022: NER is GLiNER in ONNX, optional and pinned

- **Status:** Accepted (card M3b, points 1 to 6, approved 2026-11-09; the model chosen by measurement, approved 2026-11-10); amends [ADR-0005](0005-models-are-fetched-explicitly-and-pinned-in-the-binary.md)
- **Date:** 2026-11-10
- **Deciders:** project owner
- **Related:** plan O-1, [ADR-0013](0013-a-synthetic-corpus-with-truth-and-golden-outputs-gates-recall.md)

## Context

Patterns find names that a word marks (`Frau Albers`, `Hallo Jonas
Brandt`, a column `name`). In running text most names stand alone (`Beim
Termin war auch Lena Vogt`): patterns found none of nine such names in a
synthetic test set. Only a model finds them. A model is a download of a
gigabyte, and ONNX Runtime is code veilq loads.

## Decision

- **The model:** `urchade/gliner_multi_pii-v1` (Apache-2.0, mDeBERTa-v3
  base), exported to ONNX in fp32 (1.1 GB). Chosen by measurement against
  GLiNER2-PII: equal on the corpus (person recall 0.98 at a score of 0.3, no
  false person), one false person fewer on a harder German set, one ONNX
  file instead of eight. int8 quantisation lost five of nine names; fp16 did
  not convert: fp32 until a smaller variant measures the same.
- **The detector `ner`:** the model is asked for `person`, `organization`,
  `location`; only `person` becomes a `PERSON` candidate (the other labels
  take firms and places away from it). A span without a capitalised word of
  letters, or of role words only (`Der Kunde`, `Herr Doktor`), is dropped.
  A score of at least 0.3 becomes a confidence of 0.7 + 0.3 x score, so the
  default threshold lets it through and a policy can raise it. Windows of
  200 words, 40 shared. Policy, allow lists and the overlap rule apply as to
  patterns.
- **It runs when installed.** Without it a report says `review_required`
  (names that no word marks may be left); `detectors.require = ["ner"]`
  makes a missing model an error.
- **Pinned and fetched explicitly** (ADR-0005): `veilq models fetch ner`
  downloads with `curl` and installs only what matches the SHA-256 compiled
  into the binary; `veilq models install ner --from <dir>` does the same
  from local files; `veilq models list` shows both. ONNX Runtime 1.31.1 is
  pinned the same way, as Microsoft's release archive per platform
  (Windows x64, Linux x64 and aarch64, macOS arm64) and the library in it.
  The model files are hosted as a release asset of veilq (`models-ner-1`).
- **Built without downloads:** `ort` with `load-dynamic` only, `tokenizers`
  without `http`; GLiNER's span-mode inputs and decoding are veilq's own,
  tested against a dump of the Python implementation.
- **Measured:** `veilq-eval check eval --ner --gate eval/gate-ner.toml`:
  recall 1.000 on the corpus and five held-out seeds, no false person.

## Alternatives considered

- **GLiNER2-PII:** as good on our data, eight ONNX fragments, its own
  precision reported low (0.35 on SPY).
- **`gline-rs`:** pins an older `ort` with its default features (a build
  downloads ONNX Runtime) and `tokenizers` with `http`.
- **A TLS client in veilq for `fetch`:** the SHA-256 already decides what is
  installed; `curl` is on every supported system.
- **NER on by default, a required download:** a gigabyte for users who scan
  logs and exports, where columns and keys already name the person.

## Consequences

- A run with NER loads the model first (about 3 s) and spends about 0.02 s
  per sentence on a CPU; a CSV with many cells costs a model call per cell.
- CI measures patterns only until it caches the model directory.
- The training data of the model is synthetic and its licence is not stated
  on the model card; the model's weights are Apache-2.0.

## Enforced by

- `crates/veilq-core/src/ner.rs` (inputs, decoding, filters; a test against
  the real model when `VEILQ_NER_DIR` and `VEILQ_ORT_DYLIB` are set).
- `crates/veilq-core/src/models.rs` (`PINNED`, `install`), with tests.
- `crates/veilq-core/src/engine.rs` (`with_ner`, the review reason).
- `eval/gate-ner.toml`; `crates/veilq-eval` (`check --ner`).
