# ADR-0032: The UI shows at once, and a run waits for the models

- **Status:** Accepted (a user's report via the owner, 2026-11-11: "about five seconds until the UI is there")
- **Date:** 2026-11-11
- **Deciders:** project owner
- **Related:** [ADR-0022](0022-ner-is-gliner-in-onnx-optional-and-pinned.md), [ADR-0027](0027-a-double-click-opens-the-ui-and-setup-installs-for-the-user.md), [ADR-0029](0029-veilq-shows-its-ui-in-a-window-of-its-own.md)

## Context

`veilq ui` and the double-click veilq built the engine with its models
(NER, about 600 MB, OCR and PDFium) before the server listened: 1.9 s on
the machine it was measured on, more on a slower disk or with a virus
scan, before the window could even open. Most of what a user does first
(look at the page, choose a file, open Settings) needs no model.

## Decision

- The server starts with the engine of the **patterns alone** and answers
  at once; the models load on a thread beside it and replace the engine
  when they are there.
- **A run waits for the models** (redact, a PDF's pages, a folder run): it
  never goes with the patterns alone, which would leave a name without a
  title in place. If the models do not load, a run fails with the reason
  (HTTP 503) instead.
- `/api/info` says `loading` (and `load_error`); the UI shows "Loading
  models …" until they are there.

Measured on the release build: the window shows after 0.5 s instead of
about 2.5 s; the models are there after 2.5 s.

## Alternatives considered

- **Run with the patterns while the models load:** quicker answers, but a
  redaction weaker than the one the user installed models for, without a
  sign of it.
- **Loading the models lazily on the first run:** the first run would wait
  as long, and its wait would come as a surprise.

## Consequences

- A run started in the first seconds waits; the badge says why.
- `veilq redact` and `scan` on the CLI are unchanged: they load what they
  need before they read.

## Enforced by

- `crates/veilq/src/ui.rs` (`Options::load`, `ModelState`, `ready_engine`)
- `crates/veilq/src/main.rs` (`patterns_engine`)
- `ui/src/App.vue` (the badge)
- Test: `ui::tests::a_run_waits_for_the_models_and_never_goes_without_them`
