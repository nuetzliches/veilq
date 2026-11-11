# ADR-0026: A picture is filled until a round finds nothing new

- **Status:** Accepted (card in the conversation, points 1 to 4, approved by the owner 2026-11-11)
- **Date:** 2026-11-11
- **Deciders:** project owner
- **Related:** [ADR-0003](0003-exit-codes-0-1-2-and-veilq-fails-closed.md), [ADR-0015](0015-scan-and-redact-output-and-exit-codes.md), [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md) (amended: rounds)

## Context

ADR-0023 reads a filled picture again and fills what that finds, for up to
three rounds; a fourth finding fails the run. On 115 screenshots from a
downstream legal-software stack's tickets, 12 failed so. Reading the
picture again moves its text: a filled line drops out of OCR, rows group
anew, a value's left or upper neighbour changes, and a candidate just below
the threshold (0.79 to 0.85) crosses it in the next round. In tables and
lists a round found one more row each time; at the edges of black boxes
OCR read single letters that NER took for a name. The error named neither
the place nor a way on.

## Decision

- **Rounds:** verify reads the picture again as long as each round fills
  something new, for up to **six** rounds (a GIF frame and a PDF page each
  their own).
- **When the sixth round still finds something**, it is an error as
  before, now `Unsettled`: `the picture still shows PERSON at 97×24 at
  142,228 after 6 rounds of filling; nothing was written`, with the frame or
  page where there is one.
- **`veilq redact --unverified`** (and `unverified: true` in the web UI's
  request, its "Generate anyway" button on that error, HTTP 409) fills the
  sixth round's findings too and writes the picture; the report asks for a
  review and names the first place and how many more. A value OCR found is
  never left visible; what is not checked is the picture after the last
  fill. Exit code 1. The MCP server does not offer it.

## Alternatives considered

- **Leave the sixth round's findings unfilled:** what veilq found would be
  in the output.
- **`--unverified` as the default:** a picture that was not read again
  would leave the machine without anyone asking for it.
- **More rounds without a limit:** a round costs about as much as the
  first (11 s on a 2500 px screenshot); remnants at box edges need not end.

## Consequences

- A picture that settles late takes up to six rounds. NER's candidates
  are kept per text, so a line that did not change is not read by the
  model again; a round after the first costs mostly OCR.
- `--unverified` output can hold what OCR never read; so can any picture.

## Enforced by

- `crates/veilq-core/src/visual.rs` (`ROUNDS`, `redact_picture`, `place`)
- `crates/veilq-core/src/error.rs` (`Unsettled`)
- `crates/veilq/src/main.rs` (`--unverified`), `crates/veilq/src/ui.rs`
  (409, `unverified`), `ui/src/App.vue` ("Generate anyway")
- Tests: `visual::tests::an_unsettled_picture_says_where`,
  `ui::tests::an_unsettled_picture_is_a_conflict_the_ui_can_resolve`

## What this ADR does not say

Whether the late findings are names: on the screenshots measured, some are
rows of a table, some are letters at a box's edge. Reading less of a
picture (a smaller copy, fewer frames) is not part of it.
