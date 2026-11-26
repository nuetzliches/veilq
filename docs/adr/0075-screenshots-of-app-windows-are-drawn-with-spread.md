# ADR-0075: Screenshots of app windows are drawn with spread

- **Status:** Accepted (the owner asked for this as the first step in the conversation, 2026-11-26; the card is in #169)
- **Date:** 2026-11-26
- **Deciders:** project owner
- **Related:** #169, #170, #161, [ADR-0070](0070-the-patches-of-the-detection-are-listed-switchable-and-measured-per-profile.md), [ADR-0072](0072-a-second-reading-as-handwriting-near-the-first-and-a-cut-line-over-ink.md), [ADR-0074](0074-points-from-the-review-before-0-9-0.md)

## Context

The last changes to how veilq reads pictures passed the corpus and failed
on the real screenshot they were made for. The corpus held one picture
drawn by hand per reported case (`de/png/settings-1.png` and others): a
change tuned to it measured that picture, not the class. A threshold
chosen from the drawn settings window, whose panels lay 12 to 17 off its
background, failed on the real one, whose second window lay 30 to 41 off
(ADR-0074, point 1, withdrawn). The owner asked for a corpus that holds
the spread of real screens before any new rule or model for pictures
(#170).

## Decision

1. **A generator of app windows** (`visual::ui_png`) draws, from a seed,
   one to three windows side by side: light or dark, each window a shade
   off the last (up to 40), panels 5 to 60 off their window, pale and
   muted text beside normal text, screen scaling from 100 to 200 %, and
   one of three kinds per window: an account (a picture, a person's name
   and e-mail address, at times the phone number, then a side bar of menu
   entries), settings (labels, descriptions, a field with a device's
   model), or devices (panels with models, one phone named after its
   owner).
2. **Brands and models are made up** (`Corvex`, `Nordwell Link 73`): short
   capitalised words that are no person's, in titles, fields and panels.
   Only the persons' data are truth, so a brand taken for a name counts
   as a false finding.
3. **The picture is cut** at the bottom through whatever stands there,
   at times at the top; a value the cut leaves at least three rows of is
   truth in what is left of it.
4. **Eight per corpus** (`de/png/ui-1.png` to `ui-8.png`), each from a
   stream of its own: one profile, `de/png/ui`, measured over its pictures
   and the held-out seeds 7001 to 7005. Its gate is what main reached:
   recall 1.0 on every seed, precision 0.66 (from 0.667 to 1.0 by seed, 0
   to 14 false findings: 20 of the 28 over all seeds a brand or menu entry
   the file's names repeated, 7 cut chrome filled as unread text, one
   NER alone).

## Consequences

- The profile shows both classes of the reported screenshots over a
  spread, not one picture: a brand or menu entry NER takes for a name and
  the file's names repeat (ADR-0047), and cut chrome filled as unread
  text. A change to either is now judged on this distribution and the
  held-out seeds.
- A change may raise the profile's floor, never lower it.
- The corpus grows by eight pictures; a `check` with OCR takes longer.

## Enforced by

- `crates/veilq-eval/src/generate/visual.rs` (`ui_png`),
  `crates/veilq-eval/src/generate.rs` (`UI_SCREENSHOTS`)
- `eval/de/png/ui-*.png`, `eval/gate-ner.toml`
  (`[profiles."de/png/ui"]`)
