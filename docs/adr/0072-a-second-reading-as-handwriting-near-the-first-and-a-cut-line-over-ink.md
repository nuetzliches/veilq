# ADR-0072: A second reading as handwriting counts near the first, and a cut line grows over ink only

- **Status:** Accepted (card in the conversation, six points, approved by the owner 2026-11-25; point 1 replaced after its measurement failed, the replacement approved 2026-11-26); amended by [ADR-0074](0074-points-from-the-review-before-0-9-0.md) (a cut line grows over ink 24 off)
- **Date:** 2026-11-26
- **Deciders:** project owner
- **Related:** #161, [ADR-0059](0059-a-files-values-firms-as-parties-and-lines-cut-by-the-edge.md) (amended: R4, the widening of a cut line), [ADR-0060](0060-a-line-read-badly-is-read-again-as-handwriting.md) (amended: point 3, which second readings count), [ADR-0067](0067-a-line-read-badly-is-read-again-in-black-and-white.md), [ADR-0070](0070-the-patches-of-the-detection-are-listed-switchable-and-measured-per-profile.md)

## Context

A real screenshot, reported on 2026-11-25 (two settings windows side by
side in a dark theme), showed two false fills that no rule of the
detection explains, both in the reading of the picture:

- A line of printed small print, read by OCR right but for one letter
  below `WEAK`, was read again as handwriting (ADR-0060). The recogniser
  of handwriting gave a row of zeros and nines, the phone pattern found
  it, and a box was filled over the line. One weak letter of thirty was
  enough to call the recogniser, and whatever it read counted.
- The bottom edge cut a button's label (ADR-0059, R4). Its fill grew to
  the left and right over every pixel that was not exactly the edge
  row's most common colour: the windows' panels, a few shades off, are
  such pixels, and the fill ran across the picture, 970 of 1825 pixels
  wide, over the other window.

## Decision

1. **A second reading as handwriting counts near the first (point 1,
   replaced).** It is kept where it is at most half its characters apart
   from OCR's reading (characters to change, add or remove, per character
   of the longer one; whitespace left out), or where OCR read at least a
   quarter of the line's letters and digits badly and so is no measure
   (`Line::agrees`, `APART`, `UNREAD`). Which lines are read again is
   unchanged: a line with a letter or digit below `WEAK`.
   The card's first point read again only lines with a quarter of their
   letters weak. Measured on the handwriting track with the held-out
   seeds, the hand-filled forms gave lines with one weak letter in 11 to
   31 (a share of 0.03 to 0.20), and recall fell on every seed (0.95 to
   0.85 on one). What told them apart was agreement: OCR and the
   recogniser read the corpus's handwriting at most 0.33 apart, mostly
   spaces lost, where the screenshot's two readings had next to nothing
   in common.
2. **The reading in black and white (ADR-0067) is unchanged (point 2).**
   It has the same trigger, but reads printed text with the same OCR; the
   corpus gives no false finding of it.
3. **A cut line grows over ink only (point 3).** `over_ink` counts a pixel
   as ink when a channel lies more than `INK` (60) off the background,
   the measure a box over handwriting already used (ADR-0060, point 5);
   both use `ocr::is_ink`. A surface in another shade of the background
   is no ink.
4. **Fail closed as before (point 4).** A line the edge cuts and veilq
   cannot read is filled whole as `UNREAD_TEXT`, whatever the policy says;
   only how wide the fill grows changes.
5. **Tests (point 5).** Unit tests in `ocr.rs` on synthetic input: a cut
   line's box beside a panel a few shades lighter, a reading as
   handwriting far from a line read well, near one, and one beside a line
   read mostly badly, and the distance of two readings.
6. **A profile (point 6):** `de/png/settings-1.png`, drawn by the
   generator with seed 1: two settings windows in a dark theme, an
   account with name and e-mail over panels a few shades off the window,
   one of them reaching the bottom edge, and a paired phone named after
   its owner whose line the bottom edge cuts. Its gate is what main
   reached on the corpus and the held-out seeds 7001 to 7005: recall 1.0,
   precision 0.8. The one false finding is the word before the owner's
   name, taken for a name of the file (ADR-0047); it is left to #161.

## Alternatives considered

- **Reading again only lines with many weak letters** (the card's first
  point): measured, it lost handwriting on every seed; see point 1.
- **Keeping a second reading only where it is near the first, without
  the exception for a line read mostly badly:** simpler, but a real hand
  OCR cannot read at all is read by the two models wholly apart, and that
  is where the recogniser is needed.
- **A detector of horizontal edges and surfaces** to stop a cut line's
  fill: more code for what one measure of ink already decides.

## Consequences

- A printed line OCR read well but for a letter keeps its one reading;
  a value the recogniser alone would have read there, far from OCR's
  reading, is not found from it. Where OCR read a quarter of a line
  badly, both readings count as before.
- A hand OCR reads fairly well but wrongly, with few weak letters, and
  the recogniser reads more than half apart from it: its second reading
  is dropped. The corpus shows no such line; a real one would be a miss.
- A cut line's fill no longer covers panels beside it; it stops where the
  ink ends, across gaps as wide as the line is high, as before.
- How wide a fill grows is not what the corpus measures: a fill is
  explained when it meets a truth. The width is held by the unit test.

## Enforced by

- `crates/veilq-core/src/ocr.rs` (`Line::agrees`, `apart`, `is_ink`,
  `over_ink`), tests
  `a_reading_as_handwriting_far_from_a_line_read_well_is_dropped`,
  `readings_apart`,
  `a_cut_lines_box_does_not_grow_over_a_surface_of_another_shade`
- `crates/veilq-core/src/visual.rs` (`to_ink` uses `is_ink`)
- `crates/veilq-eval/src/generate/visual.rs` (`settings_png`),
  `eval/de/png/settings-1.png`, `eval/gate-ner.toml`
  (`[profiles."de/png/settings"]`)

## What this ADR does not say

- Which lines are read again as handwriting or in black and white: as
  before.
- Anything about a word taken for a name in a window's title: the
  screenshot's report named one, and it could not be reproduced with the
  built-in policy.
