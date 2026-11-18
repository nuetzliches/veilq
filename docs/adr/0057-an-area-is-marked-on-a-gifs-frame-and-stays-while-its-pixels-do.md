# ADR-0057: An area is marked on a GIF's frame and stays while its pixels do

- **Status:** Accepted ([card](https://github.com/nuetzliches/veilq/pull/122), point 2, approved by the owner 2026-11-18, with the fill staying in the frames after it); amends [ADR-0054](0054-a-mark-acts-on-its-document-is-kept-as-a-case-and-proposes-rules.md) (an area on a GIF's frames was an error)
- **Date:** 2026-11-18
- **Deciders:** project owner
- **Related:** #114, #34, [ADR-0030](0030-a-gif-frame-is-read-where-it-changed.md), [ADR-0056](0056-an-area-takes-a-face-or-a-signature-and-the-policy-sets-how-long-excerpts-stay.md)

## Context

A screen recording is where a value veilq left is hardest to catch, and an
area drawn on it was refused: the UI showed the GIF playing, with no frame
to draw on, and an area on a GIF was an error (ADR-0054).

## Decision

- **An area belongs to the frame it is drawn on** (`frame`, from 1, as a
  PDF's page). "Mark an area" on a GIF shows its output frame by frame, as
  a still picture with a frame picker; the area is drawn on the frame shown.
  `POST /api/frame` gives one frame of a GIF as PNG and how many it has.
- **It is filled from that frame on, while its pixels stand still**, as a
  box found there would be (ADR-0030): filled on its frame, changed or not
  since the frame before, and in the frames after it until the pixels inside
  it change. It is reported once, on its frame (detector `mark`).
- **The case keeps that frame's crop**, from the input GIF; an area on a GIF
  that names no frame is refused, and one on a frame the GIF does not have
  is an error (fail closed).
- **A box on a GIF is not released from the findings**: its fill stays in
  the frames after it, which one frame's release would not reach.

## Alternatives considered

- **Filled on its frame only:** a value that stands still for a second is
  in a dozen frames; marking each is work nobody does, and the one left
  is a leak.
- **Frames decoded in the browser** (`ImageDecoder`): not in every browser
  veilq's UI runs in; the server decodes GIFs already.

## Consequences

- Showing a frame decodes the GIF up to its end on each step; for a long
  recording a step takes as long as decoding it.
- Releasing a box on a GIF, and a released area that reaches the frames
  after it, come later if asked for.

## Enforced by

- `crates/veilq-core/src/visual.rs` (`gif`, `redact_changed`, `gif_frame`)
- `crates/veilq/src/ui_cases.rs` (`takes_areas`, `area_case`, `frame`)
- `ui/src/App.vue`, `ui/src/components/ImagePane.vue`
