# ADR-0041: A cell of a table in a picture is read whole

- **Status:** Accepted
- **Date:** 2026-11-13
- **Deciders:** project owner
- **Related:** [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md), [ADR-0026](0026-a-picture-is-filled-until-a-round-finds-nothing-new.md), [ADR-0034](0034-scan-explains-what-it-read-and-why-it-decided.md)

## Context

A screenshot of a claims table from a downstream legal-software stack had a
narrow column of creditors whose names wrapped over three lines
(`KMS Bad` / `Kissinger` / `Servicegesellschaft`, synthetic here). veilq
filled the middle line black in most rows, and only it: a company name
with a hole in it, and a reader who sees that something was hidden.

ADR-0023 reads a picture's lines in rows: lines whose middles fall into
each other's height are one row of the page's text. The middle line of a
full cell sits level with the one-line cells beside it (an amount, a
rate), so the row became `Inkasso GmbH  4711023/24/0  Kissinger  2.247,43`
and NER took `Kissinger` for a surname (0.98); the first and the last line
went into rows of their own. Read as one text, the cell
(`KMS Bad Kissinger Servicegesellschaft`) holds no name.

Every detector reads a text; nothing in veilq knows a table or a column.

## Decision

1. Before the rows are formed, lines stacked closely are joined into a
   **cell**: a line right below another (a gap of at most half a line, or
   an overlap of a quarter, as OCR's boxes of close lines overlap), in the
   same type size, aligned with it on the left, the middle or the right.
   The cell's lines are one text, joined with a space; the cells then form
   the rows as the lines did.
2. A table's rules are not read, and the lines of a full cell sit as close
   to the next row's as to each other. A stack of lines is therefore **cut**
   where the one-line cells beside it show the rows: each stands in the
   middle of its row, and a stack beside two of them is cut halfway
   between.
3. The rest stays: each line is still read alone with its neighbours as
   context, and a finding keeps the boxes of its characters, one per line.
   This changes which text the page reading sees, not what is filled for a
   finding.
4. A paragraph is a stack too, and is read as one line of text, as it was
   written. The corpus gate (with NER and OCR) showed no miss and no new
   false positive.

## Alternatives considered

- **A column of companies:** recognise a column whose cells mostly carry a
  legal form (`GmbH`, `AG`) and keep its other cells. A person can stand in
  that column (a creditor can be a person), and keeping a value because of
  its neighbours is the wrong direction for a tool that fails closed. A
  cell read whole gives NER the context it needs without such a rule.
- **Drop a row's finding that the line alone does not confirm:** the line
  alone (`Kissinger`) is too short to confirm or deny anything, and
  dropping a finding because another reading missed it weakens every
  picture.
- **Detect the table's rules in the pixels:** precise where there are
  rules, nothing where a table has none or only stripes; the rows beside
  give the same cut without image analysis.

## Consequences

- A name that wraps in a cell (`Jonas` / `Brandt`) is now read as one;
  before, each half stood in another row.
- A full cell with no one-line cell beside it (every cell of the row
  wraps) is not cut, and may run into the next row's cell. The text is
  then longer, not lost: each line is still read alone.
- A label above its value in a form, set closely, is read as one text
  (`Name Jonas Brandt`), which suits the detectors' context words.
