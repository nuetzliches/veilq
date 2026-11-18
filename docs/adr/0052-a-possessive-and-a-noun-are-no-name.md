# ADR-0052: A possessive and a noun are no name

- **Status:** Accepted (card in the conversation, points 1 to 5, approved by the owner 2026-11-17); amends [ADR-0047](0047-a-name-found-once-is-found-wherever-the-file-names-it.md)
- **Date:** 2026-11-17
- **Deciders:** project owner
- **Related:** [ADR-0022](0022-ner-is-gliner-in-onnx-optional-and-pinned.md), [ADR-0045](0045-text-read-badly-is-reviewed-filled-or-stops-the-run.md)

## Context

A downstream legal-software stack redacted a screenshot of an inbox of
acknowledgements. Subjects such as `Wir haben Ihre Nachricht vom … erhalten`
and `Ihre Nachricht hat uns erreicht` came back with a black box over
`Ihre Nachricht`. NER takes a possessive and a capitalised German noun for a
person, and the names of the file (ADR-0047) learn both words, so every
`Ihre` and every `Nachricht` on the page is filled. A sign-off has the same
root: from `Ihr Jonas Brandt` the list learns `Ihr`.

## Review card

| # | Category | Point | Beats | Cost of reversing |
|---|---|---|---|---|
| 1 | Detection | Possessives (`mein…`, `dein…`, `sein…`, `ihr…`, `unser…`, `euer…`, `my`, `your`, `his`, `her`, `our`, `their`) are role words: never a word of a name, never learned into the file's names. | Today: a sign-off's `Ihr` is filled across the file. | Shorten a list. |
| 2 | Detection | A `PERSON` span of a possessive and one word with more of its line around it (`Ihre Nachricht`, `Ihr Anliegen`) is dropped with the verdict `role_words`; alone on its line (`Ihre Lena` under a greeting) the word stays a name. | A list of German nouns; dropping every span with a possessive, which loses sign-offs. | One condition; no new verdict, so the report's schema stays. |
| 3 | Detection | A name's span that starts with a possessive (`Ihr Jonas Brandt`) loses it: the box covers the name only. | A box over `Ihr`. | Low. |
| 4 | Left as is | Boxes over short codes OCR reads with low confidence under `legal` stay (ADR-0045). | An exception for short cells, which would leave handwritten initials. | None. |
| 5 | Tests | A red unit test before the fix; the corpus gains an inbox list with `Ihre Nachricht` beside a real name. | A test of the pattern level only. | Tests only. |

## Decision

`detect::POSSESSIVES` lists the possessive determiners of German and
English. `detect::role_word` is true for them as for `NOT_NAMES`; the role
word checks (`only_role_words`, NER's `can_be_name`, the words a file's
names learn, a deny value's parts) use it.

`Detector::resolve` looks at every `PERSON` candidate but a deny list's
before anything else. When its first word is a possessive and one word
follows, and the line holds more than the two, the candidate is dropped as
`RoleWords` and reports the feature `role_words`. Otherwise the possessive
is cut from its start. The cut happens before overlaps, thresholds and the
names of the file, so `Ihr` is never a finding.

## Alternatives considered

- **A dictionary of German nouns:** large, never complete, and a surname is
  often a noun (`Koch`, `Becker`).
- **Every span with a possessive dropped:** `Ihre Lena` and `Ihr Jonas
  Brandt` close many letters.
- **Articles (`die`, `der`) as well:** German speech puts an article before
  a surname (`die Albers hat angerufen`); they stay part of what NER sees.

## Consequences

- `Ihre Nachricht`, `Ihr Anliegen`, `deine Anfrage` in a sentence are no
  longer filled, and their words no longer fill the rest of the file.
- A sign-off on the greeting's line counts as alone too: the pair ending
  its line after a comma (`Viele Grüße, Ihre Lena`) keeps the name (found
  in the code review before 0.5.0).
- A possessive and a single name inside a sentence (`Grüße an Ihre Lena`)
  is dropped. A cue before it, a whole name, or the name elsewhere in the
  file still finds it.
- A line of just a possessive and a noun (`Ihre Nachricht` as a heading)
  keeps the noun as NER found it.
- The corpus holds `de/png/inbox-1.png`; a build before this change
  (0.4.0) filled both `Ihre Nachricht` subjects there, this change only the
  real name.

## Enforced by

- `crates/veilq-core/src/detect/mod.rs` (`POSSESSIVES`, `role_word`, `after_possessive`, `resolve`, `features`, tests)
- `crates/veilq-core/src/ner.rs` (`can_be_name`)
- `crates/veilq-eval/src/generate/visual.rs` (`inbox_png`), `eval/de/png/inbox-1.png`
