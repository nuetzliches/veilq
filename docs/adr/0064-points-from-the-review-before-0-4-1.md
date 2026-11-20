# ADR-0064: Points from the review before 0.4.1

- **Status:** Accepted (card in the conversation, points 1 to 5, approved by the owner 2026-11-20; point 4 not built, as its measurement decided)
- **Date:** 2026-11-20
- **Deciders:** project owner
- **Related:** [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md), [ADR-0049](0049-small-text-in-a-picture-is-read-larger.md), #89

## Context

The review before 0.4.1 left points in what #87 (ADR-0049) added; the
IBAN lengths were fixed in #128. Open were:

- An IBAN read short is widened to the right by what its lost characters
  take. The detector ends a line at a wide gap, and the rest of the IBAN
  after the gap stayed beside the widened box.
- The pattern of an IBAN glued to its label knew `IBAN`, `BAN` and `AN` in
  capitals only; `IbanDE89…` or `NDE89…` found nothing.
- A street in lower case had to be one word: `uf der Höhe 9, …` or
  `am lindenweg 12, …` left their first words.
- Every picture below 1600 px is detected at up to twice its sides, also a
  GIF band of large text.
- The noise floor of the detector, 3 x 3 cells of its probability map, was
  the same at twice the size, where it admits specks of 1.5 pixels.

## Decision

1. **The rest after a gap (point 1):** where another of the page's OCR
   boxes starts on the same row within the lost characters and two more
   from the end of an IBAN read short, the box reaches to that box's end,
   at most as far as the lost characters take from its start.
2. **Glued labels (point 2):** the label before the code is
   `(?i:i?b?a?n)`: in any case, and down to its last letter. The checksum
   keeps a word glued to a code out.
3. **Lower-case streets of several words (point 3):** a new pattern
   `address-postcode-lower-words`: a cut word of up to three letters, one
   or two of a street's small words (`an`, `am`, `auf`, `der`, ...), a last
   word of four letters or more, the number, a comma, the postcode and a
   town in capitals. `ab dem 4, 15938 …` and the verb in `wohnt am …` stay
   out.
4. **Upscaling only for small lines (point 4): not built.** The card left
   it to a measurement: read at the picture's own size first, and twice as
   large only when that found lines below 20 pixels or none. In the corpus
   the detector then read 33.5 Mpx in 96 calls against 24.8 Mpx in 48:
   nearly every picture holds small lines and was read twice. Recall was the
   same (477 of 477 with NER). A GIF of 1920 pixels is never enlarged.
5. **The noise floor (point 5):** a region of the map needs 3 cells a side
   times the factor the picture was enlarged by (6 at twice its size).

## Alternatives considered

- **Widening by a fixed share more:** reaches past the rest where there
  is no gap, and still not far enough where the gap is wide.
- **A lower-case street of any words before the number:** takes the verb
  before it (`wohnt am lindenweg 12, …`).
- **Upscaling from a size threshold of the picture's text:** needs a
  reading first; the measurement above shows what that costs.

## Consequences

- An IBAN read short whose rest stands after a gap is filled whole; a box
  starting there that holds more than the rest is filled only as far as
  the lost characters take.
- A glued label in any case is found; a street of several words in lower
  case is an address.
- Specks at twice the size are no boxes; text of less than 3 pixels was
  not readable before either.
- Detection costs what it did; the cost of enlarging stays open.

## Enforced by

- `crates/veilq-core/src/visual.rs` (`with_rest`, `same_row`), test
  `an_iban_read_short_is_filled_as_long_as_it_is`
- `crates/veilq-core/locales/intl/patterns.toml` (`iban`), test
  `an_iban_glued_to_its_label_is_an_iban`
- `crates/veilq-core/locales/de/patterns.toml`
  (`address-postcode-lower-words`), test
  `an_address_without_a_street_word_is_known_by_its_postcode`
- `crates/veilq-core/src/ocr.rs` (`noise_floor`), test
  `the_noise_floor_grows_with_the_scale`
