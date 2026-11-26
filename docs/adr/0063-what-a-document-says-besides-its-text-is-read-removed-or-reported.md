# ADR-0063: What a document says besides its text is read, removed or reported

- **Status:** Accepted (card in the conversation, points 1 to 7, approved by the owner 2026-11-20); amends [ADR-0019](0019-office-files-are-edited-in-place-every-part-read.md), [ADR-0020](0020-mail-headers-bodies-and-attachments-are-read-and-written-as-mime.md), [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md) and [ADR-0028](0028-a-user-policy-edited-in-the-ui-and-folders-redacted-whole.md); amended by [ADR-0073](0073-office-files-from-office-open-again-names-tables-and-pivot-caches.md) (fonts and calculation features are not read, a table's name is renamed)
- **Date:** 2026-11-20
- **Deciders:** project owner
- **Related:** #142, [plan.md §3.4](../plan.md#34-formats)

## Context

A detector that reads what a document shows does not see what the file
holds beside it. Checked against the code, these stayed in the output as
they were:

- **Office:** of all attributes only `author`, `initials`, `displayName`,
  `userId` and external link targets were read. A picture's alt text
  (`descr`), a tooltip, a field's instruction (`w:fldSimple w:instr`, a
  `mailto:`), a mail merge's data source, a connection string, a content
  control's tag, a sheet's name, and a pivot cache, which keeps a full copy
  of its source rows in `<s v="…"/>`, were not. Text in a worksheet outside
  its cells (header and footer, a data validation's prompt) was not read
  either. XML comments and processing instructions were skipped by the
  tokenizer and left in place. The thumbnail of the first page was read
  with OCR at a size where little is legible.
- **Mail:** attachment names were not read (a known gap); in HTML only
  `href`, `title` and `alt` were, not `src` (a tracking pixel's URL with a
  recipient's address), `data-*` or `<meta content>`; comments stayed.
  Transport headers (`Received` with `for <address>`, host names, IP
  addresses, `X-Originating-IP`, a server's internal ids) were read as
  text, but no detector takes a host name or an internal id.
- **Folder run:** a file was written under its own name and listed in
  `veilq-report.json` under it, also when it could not be read and was not
  copied: `Akte_<a name>.docx`.
- **Pictures and PDFs** drop their metadata in the rewrite, but the report
  said so only for a picture's EXIF: `scan` did not say what the file held.

## Decision

veilq reads, removes or reports what a document holds besides its text.

1. **Office attributes.** Every attribute is read as a segment of its own,
   unless it is structural: a namespace, an id (`*Id`, `r:*`, `rsid*`), a
   part's or relationship's type or target, `xml:*`. An attribute veilq
   does not know is read (fail closed); a value without a letter or digit
   is not. Reading one attribute is cheap: NER reads only text of two
   words or more, and caches what it read.
2. **Sheet names.** A sheet whose name holds personal data is renamed in
   `xl/workbook.xml`, in what Excel allows (no `\ / ? * [ ] :`, at most 31
   characters, `(2)` when the name is taken). When a formula, a defined
   name, a data validation, a chart, a pivot cache's source or a hyperlink
   names the sheet, it is an error: veilq does not rewrite formulas. A
   defined name that holds personal data is an error too. The report names
   a renamed sheet by its new name.
3. **Pivot caches.** A pivot cache's definition and records are read like a
   sheet: each value under its field's name as context. A number or date
   that is personal data becomes text (`<n v>` becomes `<s v>`); an item
   with children keeps its name. A record's index into its field's items
   (`<x v>`) is not read.
4. **Unread parts of a part.** XML comments and processing instructions
   (not the `<?xml …?>` declaration) are removed from every XML part.
   `docProps/thumbnail.*` is replaced by a white 1x1 picture of its format,
   never read. A worksheet's text outside `sheetData` is read.
5. **Mail.** Only headers that say who wrote to whom and about what are
   kept and read (`From`, `Sender`, `Reply-To`, `To`, `Cc`, `Bcc`,
   `Subject`, `Comments`, `Keywords`, `Message-ID`, `In-Reply-To`,
   `References`, `Content-Description`, `Content-Location`,
   `Content-Language`), the structural ones kept unread as before. Every
   other header is removed; one review reason counts them. A part's file
   name (`filename`, `name`, their RFC 2231 forms) is read like text,
   underscores read as spaces, and replaced in characters every file system
   takes, its extension kept: a placeholder is written `[PERSON]` (a
   pseudonym `[PERSON-0a1b2c3d]`) and read back as one, so the output
   redacts to itself. The report names the part by its new name. In
   HTML, `src`, `content` and `data-*` values are read besides `href`,
   `title` and `alt`; comments are removed, except inside a style sheet or
   a script, which older mail wraps in one: there they are read as text.
6. **Folder run.** Every folder's and file's name is read the same way; a
   file is written, listed and reported under the new path, also when it
   could not be read. Two files that would meet get `(2)` before the
   extension.
7. **Reported.** A picture with EXIF, XMP, IPTC, a PNG's text chunks or a
   JPEG's comments, and a PDF with a title, author, subject, keywords,
   attachments, a form, bookmarks or an annotation's text, has one
   `METADATA` finding (detector `metadata`). `scan` exits 1 for it.

## Alternatives considered

- **A longer list of attributes to read** (alt text, tooltips, …): every
  attribute not on it stays unread, and OOXML has thousands. Reading all
  but the structural ones fails closed.
- **Rewrite formulas that name a renamed sheet:** formula syntax across
  cells, charts, defined names and pivot sources is a parser of its own; a
  mistake breaks a workbook silently. An error asks the user to rename the
  sheet.
- **Drop a pivot cache** (`saveData="0"`): the pivot table shows nothing
  until it is refreshed, and its relationships and content types must be
  edited.
- **Keep transport headers and read them as text:** no detector takes a
  host name or an id, and a list of what to remove misses the next
  client's `X-` header.
- **Keep file names, redact only the report:** the target folder itself
  then names the people.

## Consequences

- Almost every mail now carries a review reason (`Received` is in every
  delivered mail), and its transport trail is gone: a mail redacted for a
  ticket no longer shows its route. A policy option to keep headers can
  come later.
- A picture or PDF with metadata now exits 1 on `scan`, where it exited 0.
- A workbook whose sheet named after a person is used in a formula is an
  error until the sheet is renamed.
- An escaped value in a rewritten attribute or text node writes `"` and `'`
  as `&quot;` and `&apos;`, also in text, where they were plain before.
- A GIF's comment extension is not reported (its frames are encoded anew,
  so it is gone all the same).
- Reading every attribute costs time on large documents (one detector
  call per attribute with text; not measured on large files yet). The
  corpus gate's recall and false positives did not change.

## Enforced by

- `crates/veilq-core/src/office.rs` (`structural`, `segments`,
  `Workbook::names`, `pivot_fields`, `sheet_data`, `blank_picture`), with
  tests (attributes, comments and the thumbnail; a sheet's name; a pivot
  cache; a defined name).
- `crates/veilq-core/src/xml.rs` (`comments`, `escape`), with a test.
- `crates/veilq-core/src/mail.rs` (`READ`, `redact_file_name`,
  `with_file_name`, `extended`, `html_attrs`), with tests.
- `crates/veilq-core/src/visual.rs` (`text_chunks`), with a test;
  `crates/veilq-core/src/pdf.rs` (`holds_more`).
- `crates/veilq/src/ui_settings.rs` (`target_path`), with the folder run's
  test in `crates/veilq/src/ui.rs`.

## What this ADR does not say

- That a file's name is read by `scan` or `redact` on one file: the caller
  names the output there.
- That a document's visible text is read differently: only what it holds
  beside it.
