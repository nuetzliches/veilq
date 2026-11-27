# ADR-0076: The learned head's data: values in a picture's truth and measures per candidate

- **Status:** Accepted (review card in #175, six points, approved by the owner in the conversation, 2026-11-27); this records points 3 and 4
- **Date:** 2026-11-27
- **Deciders:** project owner
- **Related:** #175, #170, #169, #35, [ADR-0075](0075-screenshots-of-app-windows-are-drawn-with-spread.md), [ADR-0047](0047-a-name-found-once-is-found-wherever-the-file-names-it.md)

## Context

#175 adds a learned head to the decider that marks weak findings as likely
false, trained on the synthetic corpus (its review card, approved). The
spike of #170 found that a small tree ensemble over cheap features of a
finding beats a vision-language judge on the corpus truth and runs where
veilq runs. Two things were missing to train it: labelled candidates of
pictures (`veilq-eval candidates` read text formats only, labelled by the
values read back from the text), and features beyond the hand rules
(`detect::FEATURES`) and the detector's confidence.

## Decision

1. **A picture's truth holds its values as drawn** (`Truth.value`,
   optional): the generator writes the string it draws for every region,
   and carries it through the cut of a UI screenshot. Text formats keep
   reading their values back from the text; a picture's metadata region
   has none.
2. **`veilq-eval candidates --ocr` reads pictures** and labels each
   candidate by those values, as text candidates are labelled. A value OCR
   read one letter off in six (a swap counting once, in words of five
   letters or more) still matches.
3. **A candidate carries named measures** (`Explained.measures`, written by
   `scan --explain --json` and `candidates`):
   - `text.len`, `text.words`, `text.caps`, `text.digits`, `text.title`:
     the shape of its text, for every candidate (`engine::text_measures`);
   - `box.x`, `box.y`, `box.w`, `box.h`, `box.below`: in a picture, the
     box around its rects in shares of the picture, set by the picture's
     reader on every reading of the same text and span (a file is read
     twice, ADR-0047).
4. **Nothing is decided by them yet.** The decider reads them in a later
   step of #175; detection and the gates are unchanged.

## Consequences

- `veilq-eval candidates eval --only de/png --ner --ocr` writes 576
  candidates of 18 pictures, 450 personal; all 247 kept ones carry their
  box. The record view's side bar entries come out as not personal.
- The pictures' truth files in `eval/` carry their values.
- A tall picture is read in bands: its `box.y` and `box.below` are shares
  of the band, not of the whole picture.
- Measures such as a line's role, its neighbours and how often a value
  repeats in the file are not yet written: the training computes repeats
  from the export; roles follow with the head if the measurement asks
  for them.

## Enforced by

- `crates/veilq-eval/src/generate.rs` (`Truth.value`),
  `crates/veilq-eval/src/generate/visual.rs` (`Canvas::pii`)
- `crates/veilq-eval/src/candidates.rs` (`near`, `--ocr`), tests
  `a_value_read_by_ocr_matches_one_letter_off_in_six`,
  `a_pictures_truth_holds_its_values`
- `crates/veilq-core/src/engine.rs` (`Explained.measures`,
  `text_measures`, `Explain::place`), test
  `a_candidate_comes_with_the_shape_of_its_text_and_its_place`
- `crates/veilq-core/src/visual.rs` (`explain_place`)
