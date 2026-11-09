# ADR-0020: Mail headers, bodies and attachments are read and written as MIME

- **Status:** Accepted (card M3a, point 5, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [ADR-0019](0019-office-files-are-edited-in-place-every-part-read.md), [plan.md §3.4](../plan.md#34-formats)

## Context

A mail carries personal data in its headers (encoded words such as
`=?utf-8?B?...?=`), in bodies encoded as quoted-printable or base64, in HTML
where a name may be split by tags, and in attachments of any kind, nested
mails included.

## Decision

veilq reads an EML as MIME and writes it back as MIME.

- Headers other than the structural ones (`Content-*`, `MIME-Version`,
  `Date`) are read decoded, as `Name: value`; a changed value is written
  again, every word that is not ASCII as a UTF-8 encoded word.
- A leaf part is decoded (base64, quoted-printable, 7bit, 8bit; UTF-8,
  US-ASCII, ISO-8859-1/15, Windows-1252) and redacted in its format: plain
  text, CSV, JSON by type or file name, a `.eml` or `message/rfc822` as a
  mail, an Office attachment as in ADR-0019. HTML is one segment over all its
  text, a block tag read as a line break, so that a name across inline tags
  is found; `href`, `title` and `alt` values are read on their own.
- A part veilq cannot read (a PDF, an archive, another charset or transfer
  encoding) is an error. Images follow `[office] embedded`.
- What changed is written with the part's transfer encoding (7bit becomes
  quoted-printable when the new text is not ASCII) and as UTF-8; the
  `Content-Type` says so.
- A finding's `part` names where: `headers`, `part 1 text/plain`,
  `part 2 kunden.csv`; its `line` is the header's number or the line in the
  decoded part. A cell of a CSV attachment carries the part as its sheet.

## Alternatives considered

- **Only the body:** headers and attachments leak.
- **A mail library that rebuilds the message:** parsing is the easy half;
  writing it back unchanged where nothing changed is what keeps a mail
  readable, and the MIME writer is small.

## Consequences

- A display name that is replaced reads `<PERSON> <<EMAIL_ADDRESS>>`; mail
  clients show it, an address parser may not.
- File names of attachments are not read (a known gap).

## Enforced by

- `crates/veilq-core/src/mail.rs`, with tests (headers, quoted-printable,
  HTML across tags, a base64 CSV, the output read again, what cannot be read).
- `eval/de/eml/` with its truth.
