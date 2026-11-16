# ADR-0049: Small text in a picture is read larger

- **Status:** Accepted (card in the conversation, points C1 to C5, approved by the owner 2026-11-16); amends [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md)
- **Date:** 2026-11-16
- **Deciders:** project owner
- **Related:** [ADR-0037](0037-a-misread-checksum-and-a-compound-label-still-redact.md), [ADR-0041](0041-a-cell-of-a-table-in-a-picture-is-read-whole.md), [ADR-0045](0045-text-read-badly-is-reviewed-filled-or-stops-the-run.md)

## Context

A screenshot of a banking form, passed on scaled down to 800 pixels, came
out of veilq with an address and an IBAN still standing. Its lines were
about eight pixels high. The detector read the picture at that size: it
split the address line in two, and of the IBAN line below it found the
label and the first digits only. What OCR did read was misread the way
small text is: the street lost its capital and first letters, `IBAN` lost
its space and was glued to the code (`ANDE89…`), and digits that ran
together were read as one. Read at twice the size, the detector found both
lines whole.

The report asked for a review (ADR-0045), but nothing was filled.

## Decision

1. **C1:** The detector enlarges a picture whose longer side is below 1600
   pixels up to twice its size (`MAX_UPSCALE`); a larger one is scaled down
   to 1600 as before. Recognition still reads each box from the picture
   itself at a height of 48.
2. **C2:** A street in lower case, one word of four letters or more, before
   a number, a comma, a postcode and a town in capitals is an address
   (`de.address-postcode-lower`, 0.8).
3. **C3:** An IBAN glued to its label (`IBANDE89…`, `BANDE89…`, `ANDE89…`)
   is an IBAN, in text and in pictures alike; the label is not part of the
   finding. No country code is `AN`.
4. **C4:** In a picture an IBAN whose checksum fails was read short: its
   last box is widened to the right by what the lost characters take, at
   the width of a read one, a space per four and one more. The length
   comes from the country code (Germany's, 22, for any other and for a code
   read wrong).
5. **C5:** A town in a table's row takes two groups of dots after it
   (`Frankfurt a. ..`): read larger, OCR read the `M.` of `Frankfurt a. M.`
   as dots of their own.

## Alternatives considered

- **Enlarge the whole picture for recognition too:** a test read better
  with the picture enlarged by Lanczos first, but three times as large
  read worse than twice; the box a recogniser sees is scaled to 48 pixels
  high anyway. Left for a measurement of its own.
- **Enlarge three or four times:** a tested three times read the street in
  lower case again and costs more time; twice was enough.
- **Fill the box beside a short IBAN:** the rest of the IBAN stood partly
  in a box read as other text, partly in no box at all.
- **Fill the whole row beside a short IBAN:** a form puts other fields on
  the row; the estimate covers the IBAN and seldom much more.
- **Leave it to `[picture] low_confidence = "box"`:** it filled the
  address line in this case, but only a piece of the IBAN line, and it is
  not the default.

## Consequences

- A small picture is read at up to four times the pixels; detection takes
  longer there.
- Text beside a short IBAN may be filled with it (a BIC behind it).
- Prose with a lower-case word before `12, 12345 Stadt` is filled as an
  address; veilq fails closed.
- The corpus pictures are all below 1600 pixels and are read enlarged now;
  the gate (`eval/gate-ner.toml`) and the held-out seeds gave the same
  result as before.

## Enforced by

- `crates/veilq-core/src/ocr.rs` (`MAX_UPSCALE`, `detect_size`), test
  `a_small_image_is_read_larger_a_large_one_smaller`.
- `crates/veilq-core/locales/de/patterns.toml` (`address-postcode-lower`,
  `address-columns`), tests
  `an_address_without_a_street_word_is_known_by_its_postcode`,
  `an_address_in_the_cells_of_a_row_is_one`.
- `crates/veilq-core/locales/intl/patterns.toml` (`iban`), test
  `an_iban_glued_to_its_label_is_an_iban`.
- `crates/veilq-core/src/visual.rs` (`with_rest`, `iban_length`), test
  `an_iban_read_short_is_filled_as_long_as_it_is`.

## What this ADR does not say

- Nothing about blurred or handwritten text: that is ADR-0045.
- The corpus has no picture of text this small yet; a synthetic one is a
  follow-up for the generator.
