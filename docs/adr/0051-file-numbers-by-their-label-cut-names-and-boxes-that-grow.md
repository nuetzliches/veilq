# ADR-0051: File numbers by their label, names cut short, boxes that grow

- **Status:** Accepted (card in the conversation, points 1 to 5, approved by the owner 2026-11-17); amends [ADR-0010](0010-built-in-patterns-are-locale-packs-in-the-policy-format.md) and [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md)
- **Date:** 2026-11-17
- **Deciders:** project owner
- **Related:** [ADR-0040](0040-purposes-are-built-in-policy-packs.md), [ADR-0045](0045-text-read-badly-is-reviewed-filled-or-stops-the-run.md), [ADR-0047](0047-a-name-found-once-is-found-wherever-the-file-names-it.md)

## Context

A downstream legal-software stack ran veilq over a screenshot of its inbox
with a dunning letter in the preview. Three things stayed visible:

- The letter's file number, `Aktenzeichen: ` and eleven digits. `de.court-file`
  knows only the court's form with a slash (`3 O 128/26`); a dunning court's
  number (`26-4319077-2-0`, often printed without hyphens) has none, and the
  word `Aktenzeichen` only raised the confidence of a match it never made.
- A record's tab cut short by the screen to `Surname,…`. The surname stood
  nowhere else on the page, so the names of the file (ADR-0047) did not know
  it, and neither a cue word nor NER had anything to go on.
- Beside and under the boxes on the letter's large bold heading, the edges
  of the letters. A box grew by a fixed 2 pixels around what OCR read.

## Review card

| # | Category | Point | Beats | Cost of reversing |
|---|---|---|---|---|
| 1 | Detection | A new pattern `de.file-number-label` takes the token after `Aktenzeichen`, `Az.`, `Geschäftszeichen`, `Geschäftsnummer` or `Unser/Ihr Zeichen` (with `:` or not) as `COURT_FILE_NUMBER` when it holds a digit, whatever its shape. | The label as a boost only, which leaves numbers without a slash. | Drop one pattern. |
| 2 | Detection | A new pattern `de.dunning-number` finds a dunning court's number (2 + 7 + 1 + 1 digits, hyphens or not) at low confidence, above the threshold with `Mahn…`, `Vollstreckungsbescheid`, `Aktenzeichen` or `Geschäftsnummer` within 48 characters before it. | A bare eleven-digit rule, which takes order and phone numbers. | Drop one pattern. |
| 3 | Detection | A new pattern `de.name-cut`: one capitalised word, a comma, maybe the start of a given name, and the dots of a cut, as `PERSON` at 0.65, in text OCR read only; above the threshold of `legal` (0.6), below the default (0.7). | A detector for a tab's person icon, or blacking out every tab bar. | Drop one pattern. A town cut alike under `legal` stays a false positive. |
| 4 | Picture output | A box grows by a quarter of its line's height to the sides and an eighth up and down, at least 2 pixels, in place of a fixed 2 pixels. | A larger fixed margin, which reaches into the line below in small text. | One function. |
| 5 | Tests | Each point has a synthetic test; the corpus gains a dunning letter on a screen. | A fixture from the real screenshot. | Tests only. |

## Decision

**File numbers.** `de.file-number-label` (confidence 0.8) matches a label
word, case-insensitive, then spaces and an optional colon, and takes the next
token: digits, letters, `.`, `/` and `-`, with at least one digit, ending in
a letter or digit (a sentence's dot is not part of it). Where `de.court-file`
matches a longer number at the same place (`3 O 128/26`), the longer one
wins as before. `de.dunning-number` (0.4, boost 0.4) needs a word before it;
eleven digits alone are not a finding.

**Names cut short.** `de.name-cut` reads text from a picture only: the
detector skips the rules in `detect::OCR_ONLY` for any other source. What it
looks for (the dots of a cut cell after a comma) is how a screen shows a
value too long for its place; written text rarely holds it. At 0.65 it is a
finding under `legal` and a candidate below the threshold elsewhere, which
`scan --explain` shows. The finding is the surname with its comma and the
letters of the given name before the cut; the dots stay.

**Margins.** `visual::grown` replaces the fixed `PAD` for every box drawn
over text OCR read and for a line read badly (ADR-0045): `max(h / 4, 2)`
to the sides and `max(h / 8, 2)` up and down, where `h` is the height of the
line's box. A line of 20 pixels gets 5 to the sides and 2 up and down; one of
40 pixels 10 and 5. Lines under 12 pixels keep 2 all round.

## Alternatives considered

- **The label as a stronger boost to `de.court-file`:** the boost cannot
  find what the regex does not match.
- **A rule per court's format:** Germany alone has dozens; the label says
  more than the shape.
- **`de.name-cut` for every source, or above the default threshold:** a
  town, a product or a status cut alike would be filled in every run, for a
  case seen in screenshots of case files.
- **A detector for a person icon beside a tab:** a picture model of its own
  for one kind of screen.
- **A larger fixed `PAD`:** what helps a 40-pixel heading reaches into the
  next line of 10-pixel text, where `covered` already guards against a box
  that only touches a value.

## Consequences

- Any token with a digit after `Aktenzeichen`, `Az.` or `Ihr Zeichen` is
  filled, also a date written there (`Ihr Zeichen: 12.03.2026`). Fail closed
  accepts that.
- Under `legal`, a capitalised word with `,…` in a picture is filled, a cut
  town or status too.
- Boxes on lines of 12 pixels and more are wider than before. Black covers a
  little more of the words beside a value; a value read again in the next
  verify round lies inside its box as before.
- The corpus holds `de/png/dunning-1.png`: a tab cut to the debtor's
  surname, the number after `Aktenzeichen:` without a slash, and an order
  number of the same shape without a label, which stays. Under the default
  policy the tab is found through the file's names (ADR-0047), since the
  letter names the debtor; `de.name-cut` alone is tested in `visual.rs`.

## Enforced by

- `crates/veilq-core/locales/de/patterns.toml` (`file-number-label`, `dunning-number`, `name-cut`)
- `crates/veilq-core/src/detect/mod.rs` (`OCR_ONLY`, `candidates`, tests)
- `crates/veilq-core/src/visual.rs` (`grown`, `boxes`, `unread`, tests)
- `crates/veilq-core/src/ocr.rs` (`Rect::pad_xy`)
- `crates/veilq-eval/src/generate/visual.rs` (`dunning_png`), `eval/de/png/dunning-1.png`

## What this ADR does not say

- That every file number is found. One without a slash after another word
  (`Akte B04711`, `Vorgang B04711`) or with none is not; a policy pattern can
  add its own label.
- That boxes stopped being rectangles around what OCR read. A letter OCR did
  not read is not covered by any margin.
