# ADR-0034: `scan --explain` says what veilq read and why it decided

- **Status:** Accepted (card in the conversation, points 1 to 5, approved by the owner 2026-11-11)
- **Date:** 2026-11-11
- **Deciders:** project owner
- **Related:** [#40](https://github.com/nuetzliches/veilq/issues/40), [ADR-0003](0003-exit-codes-0-1-2-and-veilq-fails-closed.md), [ADR-0015](0015-scan-and-redact-output-and-exit-codes.md), [ADR-0016](0016-the-mcp-server-returns-redacted-content-and-reports-only.md), [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md)

## Context

When veilq misses a value or blacks out the wrong one, nobody outside the
code can see why: what OCR read on a picture, which detector found a span,
how confident it was, and which rule dropped it (a role word, a threshold,
an overlap). Finding why a product's logo was taken for a name needed a
debug line in the code. Users and whoever supports them need that view,
and marking a gap (#34) starts from it.

## Decision

1. **`veilq scan --explain`** writes, per text veilq read (a line or a
   segment of text, a cell, a picture's OCR line and joined page, a second
   reading without formatting marks, a verify round's new text), every
   candidate with its detector, confidence, the threshold it was held to
   and the verdict: `found`, `below threshold`, `allowed`, `overlapped`,
   `role words`, `system account`, `brand word`, `placeholder` (in JSON
   `kept`, `below_threshold`, `allowed`, `overlap`, `role_words`,
   `account`, `brand_word`, `placeholder`); a birth date
   raised beside a name and a near miss (up to 0.2 below the threshold)
   are marked. Text for people; with `--json`, one line `{"path",
   "reading"}` per reading, **unstable** (v0, may change without a major
   version). A reading that repeats the one before it is written once.
2. **The explanation holds the values in clear.** It goes to **standard
   error**, only on the flag, after a line that says so; it is never part
   of the report (`--json`, `--sarif`, `redact --report`), never offered by
   the MCP server, `redact` or `mark`. `--explain` and `--sarif` exclude each
   other.
3. **It observes and changes nothing:** findings, output and exit code are
   the same with and without it. The detector decides each candidate once
   (`Detector::decide`); the findings are the candidates it keeps.
4. **The UI** asks `/api/redact` with `"explain": true` and gets the
   readings beside the result (`explain`); without it the response is as
   before. The view itself is a switch in the output pane, built on the UI
   of ADR-0033 (a separate change). The UI listens on loopback only
   (ADR-0021), so the values stay on the machine.

## Alternatives considered

- **The explanation in the report:** every consumer of the report (a CI
  log, a gateway, a ticket) would carry the values veilq exists to keep
  out.
- **On standard output after the report:** scripts that read the report
  would break.
- **A debug build or a log level:** not there when a user needs it, and a
  log tends to be kept.

## Consequences

- A user or a supporter sees in one run why a value stayed or went; a
  report about a miss can name the detector and the verdict without the
  input.
- `Detector::findings` is `decide` filtered; a change to how candidates are
  dropped shows in the explanation at once.

## Enforced by

- `crates/veilq-core/src/detect/mod.rs` (`Verdict`, `Decision`, `decide`)
- `crates/veilq-core/src/engine.rs` (`Explain`, `Reading`, `with_explain`)
- `crates/veilq/src/explain.rs`, `crates/veilq/src/main.rs` (`scan --explain`)
- `crates/veilq/src/ui.rs` (`RedactRequest::explain`)
- Tests: `engine::tests::an_explanation_changes_nothing_and_says_why`,
  `explain::tests::the_explanation_names_each_verdict_once_per_reading`,
  `ui::tests::the_explanation_comes_only_when_asked_for`
