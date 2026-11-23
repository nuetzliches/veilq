# ADR-0067: A line read badly is read again in black and white

- **Status:** Accepted (card in the conversation, points E1 to E6 approved by the owner 2026-11-23; E1 and E3 measured and dropped as E5 says); amends [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md) and [ADR-0060](0060-a-line-read-badly-is-read-again-as-handwriting.md)
- **Date:** 2026-11-23
- **Deciders:** project owner
- **Related:** [ADR-0066](0066-small-print-in-a-scanned-page-is-read-whole-and-its-glued-words-count.md), [ADR-0045](0045-text-read-badly-is-reviewed-filled-or-stops-the-run.md), [ADR-0055](0055-a-verify-round-learns-no-names-and-a-field-label-is-no-name.md)

## Context

After ADR-0066, the names veilq still lost in scanned letters stood in
coarse black-and-white scans (100 to 120 dpi), most in small print. A
comparison with Tesseract on the corpus and on coarse synthetic letters
(Tesseract itself stays out of veilq) showed where its reading did better:
it binarises a page itself and reads it as print, and read a coarse letter
in black and white whole where PaddleOCR read half of it. The card took
three ideas from it, built without a new dependency or model:

- **E1:** recognise a PDF's page from a second rendering at its scan's
  resolution (up to 400 dpi), every box staying on the 200-dpi page.
- **E2:** read a line read badly again from its box binarised by Sauvola's
  threshold, and keep both readings, as ADR-0060 keeps the handwriting
  reading.
- **E3:** decode a line read badly with a beam search and keep the two next
  best readings beside the greedy one.

E4 bounded them (a finding only another reading makes counts where the first
reading read a character badly; another reading learns no names), E5 set
the measure (each point stays only if it covers more values with no more
false findings than `main`), E6 the contract.

Each point was switched off on its own in one build (a switch for the
measurement, removed after it), on seed 1 and the held-out seeds 7001 to
7005, each with two new scan profiles, and on eleven coarse synthetic
letters:

| Switched off | Values covered (6 corpora) | False findings | Coarse letters |
|---|---|---|---|
| nothing (E1 to E4 on) | 3022 of 3027 | 54 | 60 of 66 |
| E1, E2, E3 (as `main`) | 3021 of 3027 | 54 | 60 of 66 |
| E1 | 3022 | 54 | 60 |
| E2 | 3021 | 54 | 60 |
| E3 | 3022 | 54 | 60 |
| E4 | 3022 | 54 | 60 |

Only E2 covered more: the name in the side column of the 100-dpi scan,
which `main` lost. E1 read the 300-dpi scan no better (PaddleOCR already
reads it whole at 200 dpi), E3 found nothing the greedy reading missed. E4
changed no count; it is kept as the bound on E2's noise.

## Decision

1. **A line read badly is read again in black and white (E2):** a line with
   a letter or digit OCR read below `WEAK` (0.7) is recognised a second
   time from its box binarised by Sauvola's threshold (a window as high as
   the line, `k` 0.2), and both readings are kept (`Line::contrast`). At
   most 64 lines per reading, as for handwriting; only in the first
   reading, not in a verify round, which reads the picture as written.
2. **What only it finds counts where the line was read badly (E4):** a
   finding of the second reading counts where its boxes cover a character
   the first reading read badly; elsewhere the first reading read well, and
   a reading that differs there is noise. It learns no names of the file
   (as a verify round learns none, ADR-0055).
3. **Two scan profiles in the corpus (E5):** `de/pdf/scan-fine-1.pdf` (300
   dpi, grey, a side column of 6 pt, drawn at 300 dpi) and
   `scan-coarse-1.pdf` (100 dpi, black and white), each read by a person.
4. **Contract (E6):** schema, CLI, policy and every coordinate stay as they
   are. `--explain` names the second reading `read again (contrast):`, and
   its JSON line carries `"again": "contrast"`.
5. **Dropped as measured:** E1 (a PDF's page recognised at its scan's
   resolution) and E3 (the next best readings of a beam search).

## Alternatives considered

- **Tesseract as a second reader:** it covered 45 % of the values in
  screenshots and photos against PaddleOCR's 99.7 %, and gained three of 66
  values on coarse letters, with more false findings; a native library per
  platform to build and pin. Left out by the owner's decision.
- **E1, recognising from the scan's resolution:** measured, no value more.
- **E3, a beam search:** measured, no value more; the greedy reading's
  second choices were misreadings as well.
- **Binarising every line, or the whole page:** it costs a recognition per
  line and changes what a clean screenshot reads; a line read well needs no
  second reading.

## Consequences

- A line read badly costs one recognition more (a few milliseconds), at most
  64 per reading.
- One value more in the six corpora, none more in the coarse letters: the
  side column of a 100-dpi scan, in small print, stays partly unread. The
  report says so as before (ADR-0045).
- `--explain --json` has a new optional field `again` (the format is
  unstable).

## Enforced by

- `crates/veilq-core/src/ocr.rs` (`Ocr::contrast`, `sauvola`,
  `Line::contrast`, `read_edges`), test
  `a_grey_stroke_on_grain_comes_out_black_on_white`.
- `crates/veilq-core/src/visual.rs` (`candidates`, `settle`, `unread`),
  test `a_reading_in_black_and_white_counts_where_the_line_was_read_badly`.
- `crates/veilq-core/src/engine.rs` (`ocr_findings_read_again`, `Pass`),
  test `another_reading_of_a_line_learns_no_names`.
- `crates/veilq-eval/src/generate/visual.rs` (`FINE`, `COARSE`),
  `eval/de/pdf/scan-fine-1.pdf`, `eval/de/pdf/scan-coarse-1.pdf`, gated by
  `eval/gate-ner.toml`.
