# ADR-0074: Points from the review before 0.9.0

- **Status:** Accepted (the owner chose the points to fix before 0.9.0 in the conversation, 2026-11-26, and the edge's threshold of 24); amends [ADR-0072](0072-a-second-reading-as-handwriting-near-the-first-and-a-cut-line-over-ink.md) (point 3) and [ADR-0073](0073-office-files-from-office-open-again-names-tables-and-pivot-caches.md)
- **Date:** 2026-11-26
- **Deciders:** project owner
- **Related:** [ADR-0047](0047-a-name-found-once-is-found-wherever-the-file-names-it.md), [ADR-0069](0069-points-from-the-review-before-0-8-0.md), #156, #157, #158

## Context

The code and security review of `v0.8.0..HEAD` (documented internally),
which held the reading of a line cut by a picture's edge (ADR-0072) and
the tables and pivot caches of a workbook (ADR-0073), found these points:

1. The fill over a line the edge cuts grew over ink 60 off the background
   only (ADR-0072): pale text, `#C8C8C8` on white (55 off), was none, and
   the end of a name stayed visible beside the fill.
2. A pivot cache over a table names it in its `worksheetSource name`; no
   check saw it, and a renamed table left the cache pointing at nothing.
3. Whether a formula names a table or a column was a plain `contains`:
   `Tabelle1` was found in `Tabelle10[…]`, and a column `Akte's` was not
   found in `[Akte''s]`, as Excel writes it.
4. A `feature` element's `name` was left unread in every part, not only
   in Excel's calculation features: `<feature name="Ole Faber"/>` in a
   custom XML part passed in clear.
5. Two column or field names that would become one were an error in a
   redaction only; `scan` reported the file as fine.
6. The names of tables, columns and fields were read twice (to decide,
   and as attributes), and a lifted candidate counted twice (#129).
7. A pivot cache's records were tokenized again in a scan, and its
   definition rewritten (its quotes normalized) where nothing changed.
8. A pivot cache's values were matched to their findings by a walk over
   all findings: a cache of a million values made 10¹² steps (security).
9. Every formula was put in lower case again for every name checked
   (security, together with 3).

A tenth point, that a table's name was decided before the cells taught the
file's names (ADR-0047), was not reproduced: a redaction reads a file
twice, and the second reading knows them. A table is now read after the
cells all the same, and only in a workbook that has one.

## Decision

1. **A cut line grows over ink 24 off** (`EDGE_INK`): pale text is ink, a
   panel a few shades off the background (12 to 17 in the reported dark
   theme) is none. Elsewhere ink stays 60 off (`INK`).
   **Withdrawn after 0.9.0 (2026-11-26):** the 12 to 17 came from the
   corpus's one drawn settings window, not from the reported screenshot,
   whose second window's panels lay 30 to 41 off the first's background;
   with 24 they were ink again and the fill ran across the picture, as
   before ADR-0072. A cut line grows over ink 60 off again; pale text cut
   by the edge is the limit that stays. No threshold separates both
   cases; the corpus that would show it is #169.
2. **`worksheetSource name` is a formula's text** (`formula_texts`).
3. **A name is matched as a name** (`names_table`, `names_column`): a
   table only where no letter, digit, `_`, `.` or `\` stands beside it; a
   column in a structured reference with Excel's escapes, quoted, or as a
   name of its own. The formulas are put in lower case once.
4. **A calculation feature's name is left unread in `xl/workbook.xml`
   only.**
5. **A scan fails where a redaction would** when two names would become
   one.
6. **A name read to decide is read uncounted** (`findings_uncounted`); the
   part's own reading reports it.
7. **A scan leaves a pivot cache's fields alone**, and a `sharedItems` tag
   nothing changes in stays byte for byte.
8. **A value finds its finding by binary search.**

## Consequences

- A pale name cut by the edge is filled to its end; a panel beside it
  still is not.
- A workbook whose pivot cache sits on a table named after a person is an
  error, as one whose formula names it.
- `scan` and `redact` agree on which workbooks they refuse.

## Enforced by

- `crates/veilq-core/src/ocr.rs` (`EDGE_INK`, `ink_by`), test
  `a_cut_lines_box_grows_over_pale_letters`
- `crates/veilq-core/src/office.rs` (`formula_texts`, `names_table`,
  `names_column`, `named_elsewhere`, `table_names`, `note_fields`,
  `shared_items`), tests `a_pivot_cache_over_a_table_names_it`,
  `formulas_name_a_table_or_column_as_a_name_of_its_own`,
  `a_feature_outside_the_workbook_part_is_read`,
  `a_scan_fails_where_the_redaction_would`,
  `a_pivot_cache_without_findings_stays_byte_for_byte`,
  `a_table_named_after_a_name_the_cells_hold_is_renamed`
