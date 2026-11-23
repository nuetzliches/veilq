# ADR-0066: Small print in a scanned page is read whole, and its glued words count

- **Status:** Accepted (card in the conversation, points D1 to D4 approved by the owner 2026-11-23, D5 added and approved the same day); amends [ADR-0047](0047-a-name-found-once-is-found-wherever-the-file-names-it.md) (a word glued to the word before it) and [ADR-0049](0049-small-text-in-a-picture-is-read-larger.md) (a PDF's page)
- **Date:** 2026-11-23
- **Deciders:** project owner
- **Related:** [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md), [ADR-0045](0045-text-read-badly-is-reviewed-filled-or-stops-the-run.md), [ADR-0051](0051-file-numbers-by-their-label-cut-names-and-boxes-that-grow.md)

## Context

A scanned letter from a downstream legal-software stack came out of veilq
0.7.0 with a person's name standing twice: whole in the small print of the
letter's side column (about 6 pt), and in part in the body, where a box
covered the given name and left the rest of the name beside it. The report
asked for a review (lines read badly, ADR-0045), but nothing more was
filled.

Synthetic letters scanned at 100 to 150 dpi, black and white, showed the
same, and why:

- veilq renders a PDF's page at 200 dpi: an A4 page is 1654 x 2339
  pixels. The text detector reads every picture at a longer side of 1600
  at most, so the page was scaled to 0.68 first; the small print's lines
  came to the detector about seven pixels high.
- OCR reads small print without its spaces: `fürVorname`, a street glued
  to its number (`Lindenweg12`). The file's names (ADR-0047) are found as
  whole words, and the address patterns want a space before the number.
- A surname read badly enough (`Ooemoz` for a name of eight letters) is no
  name to any detector, and stood beside the given name filled.

With the detector reading the page at its own size (measured with a switch
built for the measurement and removed after it), the side column of the
synthetic letters was found where it was lost at 1600. The same
reading once lost the space between a street and its number in a
recipient's address, and with it the address: D5.

## Decision

1. **D1, a name glued to the word before it:** in text read by OCR, a word
   of the file's names (ADR-0047) whose capital follows a small letter
   (`fürJonas`, `gegenJonas`) is that word, detector `names:file`, as the
   cut words of ADR-0047 A4 are. Inside a word (`Lebensjonas`), after a
   capital, and in written text it is none.
2. **D2, a word read badly beside a name:** in a line read by OCR, the word
   right before and the word right after a `PERSON` finding is filled with
   it when it starts with a capital, has three letters or more and OCR read
   one of its letters or digits below `WEAK` (0.7): detector
   `ocr:name-neighbour`, at the finding's confidence. One word on each
   side, none across a glued word, none read well.
3. **D3, a PDF's page read whole:** the detector reads a PDF's page larger
   than 1600 pixels at a longer side of up to `PAGE_SIDE` (2400): an A4
   page at 200 dpi whole, a larger one scaled down to 2400. A smaller page
   and every other picture are read as before.
4. **D4, scanned letters in the corpus:** `de/pdf/scan-office-1.pdf` (150
   dpi, grey, JPEG quality 40), `scan-fax-1.pdf` (200 x 100 dpi, black and
   white) and `scan-copier-1.pdf` (120 dpi, grey, quality 35): one letter
   with a recipient's name and address, a side column in small print that
   names a second person, and a sentence that names them again. Each is a
   scan a person still reads, drawn at 200 dpi and set into the PDF at the
   scan's resolution.
5. **D5, a street glued to its number:** in text read by OCR, an address of
   one line (`Lindenweg12, 28195 Bremen`) or of two (`Am Anger7` /
   `15938 Lindau`) whose number follows the street's last small letter is
   an address (`de.address-glued`, `de.address-lines-glued`, 0.85). In
   written text a glued street and number is a code and stays.

## Alternatives considered

- **Read every picture larger than 1600 whole:** a wide screenshot (2560 x
  1440) would cost the detector the same again without small print to
  gain; a PDF's page is the picture that holds print at its size.
- **A corpus letter worsened until 0.7.0 fails it:** tried first (one page
  at 120 dpi, black and white, a threshold chosen for it). It measured the
  bug, not scans: at one threshold it was unreadable, at the next 0.7.0
  read it whole, and between seeds its recipient's address came and went.
  The three scan profiles measure kinds of scan; the unit tests hold the
  rules.
- **`[picture] low_confidence = "box"` by default:** it fills every line
  read badly whole; on a scanned letter that is most of the page.
- **Fill the whole line around a name read in part:** a line of a body
  holds more than the name; D2 fills the word beside it only when OCR
  itself says it read that word badly.
- **A detector name of its own for D1:** the file's list must not learn
  from its own findings; a second name would have to be excluded wherever
  `names:file` is. The cut words of ADR-0047 A4 set the precedent.

## Consequences

- The detector reads an A4 page at about 3.9 million pixels instead of 1.8:
  some twice the work of detection per page. Recognition, which reads each
  line from the page itself at a height of 48, does not change.
- A capitalised word read badly beside a name is filled with it (`Herrn`
  before it, a noun after it): veilq errs on filling a word too many
  (ADR-0047 A6).
- Of two words of a name glued together (`JonasBrandt`) only the second
  is found, and a word glued after a capital or a digit not at all: only a
  capital after a small letter counts.
- The three scans pass on 0.7.0 as well: they guard what is read now, and
  are the ground a later change of the reading is measured on. With this
  change, seed 1 and the held-out seeds 7001 to 7005 cover every value of
  them as before, with fewer false findings (seed 1: 11 false findings in
  the corpus, 14 on 0.7.0).
- NER read alone takes a capitalised word that starts a line of a letter
  (`Ihre Anfrage`, `Ihnen die …`) for a person, on 0.7.0 as well, and the
  file's names then fill it everywhere. The corpus letters start no line of
  their body so; that reading is a finding of its own, not this ADR's.
- Scanned again after it was redacted, the fax letter gives one false
  finding: a line its handwriting reading misread, which NER takes for a
  name. Nothing of a value is left in it.

## Enforced by

- `crates/veilq-core/src/ocr.rs` (`PAGE_SIDE`, `detect_size`), test
  `a_pdfs_page_is_read_at_its_size_up_to_the_page_side`;
  `crates/veilq-core/src/pdf.rs` (`redact`).
- `crates/veilq-core/src/names.rs` (`Inner::cut`, `Names::candidates`),
  test `a_word_glued_to_the_word_before_it_is_found_in_a_picture`.
- `crates/veilq-core/src/visual.rs` (`weak_neighbours`,
  `NEIGHBOUR_DETECTOR`, `candidates`), test
  `a_word_read_badly_beside_a_name_is_filled_with_it`.
- `crates/veilq-core/locales/de/patterns.toml` (`address-glued`,
  `address-lines-glued`), `crates/veilq-core/src/detect/mod.rs`
  (`OCR_ONLY`), test
  `a_street_glued_to_its_number_is_read_from_a_picture_only`.
- `crates/veilq-eval/src/generate/visual.rs` (`scanned_letter_pdf`,
  `OFFICE`, `FAX`, `COPIER`), `eval/de/pdf/scan-*-1.pdf`, gated by
  `eval/gate-ner.toml`.

## What this ADR does not say

- How a picture should be read in general: several readings at sizes fitted
  to the height of its lines, a PDF rendered at its scan's resolution, and
  one decision over all readings of a document are a question of their
  own, measured on the scans of D4.
