# ADR-0073: Office files from Office open again: fonts, tables and pivot caches

- **Status:** Accepted (card in the conversation, six points, approved by the owner 2026-11-26; point 4 replaced and points N2 to N4 added after a round trip through Excel, approved the same day)
- **Date:** 2026-11-26
- **Deciders:** project owner
- **Related:** #156, #157, #158, [ADR-0063](0063-what-a-document-says-besides-its-text-is-read-removed-or-reported.md) (amended: which attributes are read, a table's name)

## Context

The review before 0.8.0 found three faults in what ADR-0063 brought to
xlsx (#156, #157, #158). Their fixes were checked in Excel, not only in
unit tests, and that check found more, all from the same cause: an
attribute the format needs for its structure was read as text and
replaced.

- **Every workbook Excel saved** no longer opened after `veilq redact`
  0.8.0. Its theme names fonts (`typeface="MV Boli"`, `"Iskoola Pota"`),
  which NER took for persons, and their PANOSE classification
  (`panose="02110004020202020204"`), which the phone pattern took for a
  number; a calculation feature (`<xcalcf:feature
  name="microsoft.com:Single"/>`) was taken for a person too. Word and
  PowerPoint write the same theme.
- **Every table** was renamed: `displayName` is in the attributes that
  always name a person (a comment author's), and `<PERSON>` is no name
  Excel allows for a table.
- **A table column's name** (#156) was replaced like any attribute, so two
  columns named after persons became one `<PERSON>`, and a formula that
  names the column no longer resolved.
- **A pivot cache's field** whose numbers became text (#157) kept its
  flags (`containsString="0"`, `containsNumber="1"`) and its
  `minValue`/`maxValue`, which hold the values themselves; one was left
  unreplaced in clear. Excel rejected the cache.
- A worksheet was tokenized twice to read its attributes, and every
  attribute of every cell built a segment that was thrown away (#158).

## Decision

1. **Font attributes and calculation features are not read (N2).**
   `typeface`, `panose`, `pitchFamily` and `charset` are structural, as
   ids and relationship types are (ADR-0063); a `feature` element's
   `name` is too.
2. **A table's name is renamed, as a sheet's (N3).** A table's `name` and
   `displayName` are no person's; they are read where tables are named
   (`table_names`): one that holds personal data becomes a name Excel
   allows (`Akte_Herr_PERSON_`: letters, digits, `_` and `.`, a letter or
   `_` first, no cell reference), unique in the workbook, and an error
   when a formula names it.
3. **A column's or pivot field's name (#156, points 1 and 2).** One that
   holds personal data is an error when a formula names it (structured
   references, calculated columns, totals rows, a calculated field) or
   when, replaced, it would be the same as another name of its table or
   cache; otherwise it is replaced as before.
4. **A pivot field's flags follow its values (#157, points 3 and N1).** A
   field with a finding loses `minValue`, `maxValue`, `minDate` and
   `maxDate`. A field whose numbers or dates became text loses the flags
   that say it holds no text; where numbers or dates are left it says it
   holds mixed types, where none are it loses `containsNumber`,
   `containsInteger` and `containsDate`. A cache's definition is written
   after its records, which say what became of its fields. The card's
   point 4 kept the number flags in every case; Excel rejected a field
   that says it holds numbers and holds none.
5. **A worksheet is tokenized once (#158, point 5):** its attributes
   outside `sheetData` are read from the same tokens, `sheetData` left
   out.
6. **Evidence (points 6 and N4):** unit tests in `office.rs` for each
   point; a round trip through Excel (a table with a column named
   after a person, a pivot cache over tax ids) and PowerPoint (a slide
   with a name and a phone number): each output opens, the table and the
   pivot table are there, the pivot cache refreshes. A Word round trip
   could not run: Word stopped at a dialog before it saved the input.

## Alternatives considered

- **An error for every table name with a finding:** NER takes a default
  name such as `Tabelle1` for a person; every German table would fail.
- **Not reading a table's name:** a table named after a client would keep
  the name.
- **Renaming a column apart (`<PERSON> (2)`):** its header cell holds the
  same text and is replaced on its own; they would no longer match.
- **Counting whether every value of a pivot field became text** in one
  part: the values are spread over the definition and the records.

## Consequences

- Files Office saved open after a redaction again; 0.8.0's output of most
  of them did not.
- A font named after a person (rare; a custom handwriting font) is no
  longer read.
- A workbook with two table columns or pivot fields named after persons
  cannot be masked; pseudonymized, the names stay apart.
- A table named after a person gets a name with `_` for its spaces and
  placeholder brackets.

## Enforced by

- `crates/veilq-core/src/office.rs`: `structural`, `named_elsewhere`,
  `table_names`, `table_name`, `note_fields`, `shared_items`,
  `segments_of`; tests `fonts_and_calculation_features_are_not_read`,
  `a_tables_display_name_is_no_persons`,
  `a_table_named_after_a_person_gets_a_name_excel_allows`,
  `a_tables_new_name`,
  `a_table_column_named_after_a_person_that_a_formula_uses_is_an_error`,
  `two_table_columns_that_would_become_one_are_an_error`,
  `a_pivot_field_whose_numbers_become_text_says_so`

## What this ADR does not say

- Which other attributes Office needs as they are: these were the ones
  the round trip found. A round trip through Office belongs in the review
  of any change to what is read.
