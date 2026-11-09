# ADR-0019: Office files are edited in place, every part read

- **Status:** Accepted (card M3a, points 1 to 4 and 6, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [ADR-0003](0003-exit-codes-0-1-2-and-veilq-fails-closed.md), [plan.md §3.4](../plan.md#34-formats)

## Context

A DOCX, XLSX or PPTX is a ZIP of XML parts. What a reader sees is a fraction
of what it holds: tracked deletions, comments, headers and footers, hidden
sheets, cached formula values, speaker notes, chart labels and the
properties that name who wrote it. Word splits a word into runs after every
edit, so `Mira Albers` may be `Mi`, `ra Al`, `bers` in three elements.
Rebuilding the document from extracted text loses its layout.

## Decision

veilq reads every XML part of the ZIP and edits the text in place; parts
without a finding stay byte for byte, the ZIP keeps its order.

- A paragraph (`w:p`, `a:p`, a shared string `si`) is one segment over all
  its runs; a line break or tab between runs reads as one. A replacement goes
  into the run where its finding starts, the other runs lose their part of
  it; every run keeps its formatting. Text elsewhere (a chart label, a
  property) is a segment of its own; an external link target is read.
- Who wrote and edited the file is replaced whatever it holds (detector
  `metadata`): `creator`, `lastModifiedBy`, `Manager` and comment authors as
  `PERSON`, `Company` as `ORGANIZATION`, custom properties as `METADATA`,
  and the attributes `author`, `initials`, `displayName`, `userId`.
- XLSX: a cell is read with its column's name (row 1) as context. A shared
  string is read in every context it is shown in and replaced once; a number
  or date that is personal data becomes an inline string; personal data in a
  formula, or as a formula's result, is an error.
- Images and embedded objects are not read until M4: an error, or with
  `[office] embedded = "remove"` images are blanked (a 1x1 image of their
  format) and the report asks for a review. A nested Office file (a chart's
  workbook) is redacted like the outer one; printer settings are kept and
  named in the review reasons; other objects stay an error.
- A finding's location: `part` is the XML part, `line` the number of the
  segment in it, `start`/`end` and `column` in the segment's text; for XLSX a
  cell with its sheet's name.
- The XML is read by a small tokenizer of veilq's own (exact byte ranges for
  in-place edits), the ZIP with `zip` (MIT); `quick-xml` from the card was
  not needed.

## Alternatives considered

- **Extract text and write a new document:** layout, styles and everything
  veilq does not understand are lost.
- **Only `word/document.xml` and the sheets:** comments, deletions and
  properties leak.
- **Edit shared strings per cell:** the same string shown elsewhere keeps the
  value; rewriting the string once is what the cells show.

## Consequences

- A redacted run may hold an empty `<w:t></w:t>`; Word reads it.
- An Excel date is a number: a birth date stored as a date is found only
  under a column that says so, and a date format is not read.
- The UI shows text formats only; Office files go through the CLI and MCP
  (`output_base64`).

## Enforced by

- `crates/veilq-core/src/office.rs`, `crates/veilq-core/src/xml.rs`, with
  tests (a name across runs, metadata, comments and deletions, XLSX contexts
  and hidden sheets, images).
- `eval/de/docx/`, `eval/de/xlsx/` with their truth; `crates/veilq-eval`.
