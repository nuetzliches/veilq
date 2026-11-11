# ADR-0030: A GIF frame is read where it changed

- **Status:** Accepted (card in the conversation, points 1 to 6, approved by the owner 2026-11-11)
- **Date:** 2026-11-11
- **Deciders:** project owner
- **Related:** [#12](https://github.com/nuetzliches/veilq/issues/12), [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md) (amended: GIF), [ADR-0026](0026-a-picture-is-filled-until-a-round-finds-nothing-new.md)

## Context

ADR-0023 reads every GIF frame whose pixels changed, whole, and verifies
it for up to six rounds (ADR-0026). A screen recording of 717 frames at
3815x1958 took 26 minutes, though most of its frames change in a small
place: a pointer, a typed word, a clock. OCR and NER cost about the same
for every line of a frame, changed or not.

## Decision

- **Bands:** a frame that changed against the frame before is read only in
  bands of rows around the rows whose pixels changed: as wide as the
  frame, so a label left of a value is in it, and at least 64 rows (or a
  twentieth of the frame's height) higher above and below, so a label
  above it is too. Bands that overlap are one. (The card said 32-pixel
  tiles grouped into clusters; a band as wide as the frame makes that the
  same as rows.)
- **A cut line is read again:** when OCR reads a line within two pixels of
  an edge of a band that has rows beyond it, the band grows by the margin
  on that side and every band of the frame is read again; nothing of the
  reading given up reaches the frame or the report.
- **Whole:** the first frame is read whole, and so is a frame whose bands
  come to more than half its height (a scroll, a new page).
- **Outside the bands** every box of the frame before stays, as its
  pixels stand still (ADR-0023); the verify rounds of ADR-0026 run per
  band, and a finding names its place in the frame.
- **Measured** on a synthetic recording (`veilq-eval recording --seed 1`:
  131 frames at 1920x1080, one palette, a pointer, a clock, a scrolled
  list, a typed note, a pasted mail address, a changed phone number):
  `redact` 56 s before, 39 s after; `scan` with NER 86 s, 38 s; OCR alone
  34 s, 14 s. Recall 253 of 253 both times; the corpus gate stays green.

## Alternatives considered

- **Read the changed box only:** cuts off a label left of the value and
  lines at its sides.
- **Read every changed frame whole (ADR-0023):** 26 minutes for a long
  recording.
- **A whole read every n frames as a net:** a value it would catch is
  visible in the frames before it all the same; it only costs time.

## Consequences

- **A value outside the bands that becomes personal data only through
  context appearing in a band** (an old number, a label typed far away
  from it) is not found in that frame: its rows did not change and are
  not read. A label within the margin is read with it.
- A change is a change of any pixel. A GIF whose encoder gives each frame
  its own palette changes still pixels too (the corpus recording does);
  those rows are read, and such a GIF gains less.
- NER keeps its candidates per text; a band's text changes as the band
  moves, so NER is the larger part of what a band read still costs.
- A frame's report names a still value once, in the frame it appeared or
  changed in, not in every frame it stands in.

## Enforced by

- `crates/veilq-core/src/visual.rs` (`changed_bands`, `redact_changed`,
  `settle`, `MARGIN`, `EDGE`)
- `crates/veilq-eval/src/generate/visual.rs` (`long_recording_gif`),
  `crates/veilq-eval/src/main.rs` (`recording`)
- Tests: `visual::tests::a_frame_is_read_in_bands_around_what_changed`,
  `visual::tests::a_box_stays_while_its_pixels_stand_still`

## What this ADR does not say

How long the 717-frame recording now takes: it is not in the repository,
and its speed depends on how much of each frame changes. Nothing about
PDF pages or still images, which are read whole as before.
