# ADR-0059: A file's values, firms as parties, and lines cut by the edge

- **Status:** Accepted (card in the conversation, points R1 to R5, approved by the owner 2026-11-18); amends [ADR-0047](0047-a-name-found-once-is-found-wherever-the-file-names-it.md) (the list holds values and finds a name read one letter off) and [ADR-0045](0045-text-read-badly-is-reviewed-filled-or-stops-the-run.md) (a line cut by the edge is filled whatever the policy says)
- **Date:** 2026-11-18
- **Deciders:** project owner
- **Related:** [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md), [ADR-0042](0042-a-column-of-a-list-names-what-its-cells-hold.md), [ADR-0051](0051-file-numbers-by-their-label-cut-names-and-boxes-that-grow.md)

## Context

A screenshot of a record in a downstream legal-software stack, redacted
with a release before 0.5.0, showed four kinds of gaps. Each was rebuilt
with synthetic pictures and reproduced on 0.5.0:

- The creditor, a firm named after its owner, was filled in the field
  labelled `Gläubiger` and left in the page's heading and in the record's
  basic data. With its legal form (`Kessler Hausverwaltung GmbH`) no
  detector found it anywhere: `name-field` takes plain words only, and a
  firm is not a person to NER.
- The file's list (ADR-0047) holds persons only. A file number found
  beside its label stood unfound in a journal's line without one.
- The list matches exactly. OCR reads a link's given name one letter off
  (`Severln`), and the word stood unfilled.
- A sender's name under the greeting, cut in half by the window's bottom
  edge, was not read at all: the text detector gives no box for half a
  line at an edge, so neither a finding nor a review reason came of it.

## Decision

1. **Firms as parties (R1):** under a party's label (`Gläubiger`,
   `Schuldner`, `Gegner`, `Mandant`, `Absender`, `Empfänger`, each also
   feminine) the whole value with a legal form (`GmbH`, `GmbH & Co. KG`,
   `UG`, `AG`, `KG`, `OHG`, `GbR`, `e.K.`, `PartG`) is a `PERSON`, pattern
   `de.party-firm`, weighed as `name-field`: 0.35 alone, 0.85 with the
   label beside or above it (a picture's field has one, not only a list's
   column). Its words go on the file's list, its legal form does not. No
   new type: a policy's actions stay as they are.
2. **A file's values (R2):** every other finding kept at or above its
   threshold, but an IPv4 address, whose value has five characters or more
   without spaces and one of them a digit or an `@`, goes on the list.
   Every text read after that finds it whole (not inside a longer run of
   letters or digits), any case, a space or none between any two
   characters, as its type at its confidence, detector `values:file`. A
   value learned after it stood unfound makes the run late, as a name does.
   `[names] values = false` switches it off. An IPv4 address stays off: it
   has a version number's shape and is told from one only by a word such
   as `client` before it, which another place of the file lacks.
3. **A name read one letter off (R3):** in text read by OCR, a word one
   edit (a letter changed, added, dropped, or two swapped) off a word of
   the list of six letters or more, with the same first letter, is that
   word, at nine tenths of its confidence. Written text is read as
   written. `[names] similar = false` switches it off.
4. **Lines cut by the edge (R4):** OCR reads the 48 rows at a picture's top
   and bottom edge again with 48 rows of the edge's background beyond them
   and the lines read already painted over (the rest of a cut line
   otherwise joins the box of the line above). A box there that reaches the
   edge and no line read holds is a line the edge cuts, widened over the
   ink beside it in its rows. Read well, it is read like any other line;
   read badly or not at all, it is filled whole as `UNREAD_TEXT`, detector
   `ocr:cut-edge`, whatever `[picture] low_confidence` says: what is left
   of it cannot be read better.
5. **More filled, a false one too (R5):** a firm `Kessler …` fills `Kessler`
   wherever the file holds it, and a value found falsely is found falsely
   wherever it stands, as ADR-0047 A6 has it for names.

When two findings of a workbook's readings are alike in span and
confidence, the one a detector found itself wins over the list repeating
it; a list's candidate that only repeats a candidate as strong on its span
is not added at all.

## Alternatives considered

- **A type `ORGANIZATION`:** a contract change in the report's schema and
  in every policy's actions, for a value that is filled all the same.
- **Firms under a creditor's or a debtor's label only:** the corpus's
  client firm (`Nordwerk GmbH` under `Mandant`) was not personal data
  before; the owner chose that a firm as any party is (its truth follows).
- **Every type on the list:** an IPv4 address carried a version number's
  shape into every build number of a log.
- **Similar words in written text too:** a typo is rarer there than a
  misread in a picture, and every word one letter off a name would be
  filled.
- **A review reason for a cut line:** the person reviewing sees half a line
  and cannot read it either; filling it is the fail-closed side.

## Consequences

- A creditor or debtor that is a firm is filled in its field, its heading
  and its lists.
- A file number, an IBAN or a phone number found once is filled wherever
  the file holds it, also in a line that gives no hint of what it is.
- A word in a picture one letter off a name is filled, a word that is no
  name too (`Bäcker` beside `Becker`).
- Two more readings of the detector on narrow strips per picture, and a
  black box at the edge where a line was cut, also over a cut line that
  held nothing personal.
- The corpus's client firms under `Mandant` are personal data in its
  truth.

## Enforced by

- `crates/veilq-core/locales/de/patterns.toml` (`party-firm`), test
  `a_firm_under_a_partys_label_is_found_whole` in
  `crates/veilq-core/src/detect/mod.rs`
- `crates/veilq-core/src/names.rs` (`VALUES_DETECTOR`, `LEGAL_FORMS`,
  `one_edit`), tests
  `a_value_found_once_is_found_wherever_the_file_holds_it`,
  `a_value_learned_after_it_stood_unfound_is_late`,
  `a_name_read_one_letter_off_is_the_name_in_a_picture`,
  `role_words_brands_and_particles_stay_off_the_list`
- `crates/veilq-core/src/engine.rs` (`decisions`, `new_names`)
- `crates/veilq-core/src/ocr.rs` (`Line::cut`, `cut_lines`, `over_ink`,
  `holds`), tests `a_cut_lines_box_grows_over_its_ink`,
  `a_line_holds_a_cut_line_by_half_its_height`
- `crates/veilq-core/src/visual.rs` (`unread`, `CUT_DETECTOR`), test
  `a_line_the_edge_cuts_is_filled_when_it_cannot_be_read`
- `crates/veilq-core/src/office.rs` (`Workbook::shared_strings`)
- `crates/veilq-core/src/policy.rs` (`Names`), `schema/policy.schema.json`
- `crates/veilq-eval/src/generate/visual.rs` (`list_png`),
  `eval/de/png/list-1.png.truth.json`
