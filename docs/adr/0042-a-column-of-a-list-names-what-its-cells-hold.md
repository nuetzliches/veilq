# ADR-0042: A column of a list names what its cells hold

- **Status:** Accepted (card in the conversation, points 1 to 5, approved by the owner 2026-11-13); amends [ADR-0011](0011-policy-patterns-take-context-and-a-validator-a-policy-names-its-locales.md) (a rule at 0) and [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md) (a line's context)
- **Date:** 2026-11-13
- **Deciders:** project owner
- **Related:** [ADR-0012](0012-a-failed-check-lowers-the-confidence-it-does-not-drop-the-finding.md), [ADR-0025](0025-an-address-cut-short-is-still-an-address.md), [ADR-0041](0041-a-cell-of-a-table-in-a-picture-is-read-whole.md)

## Context

A screenshot of a case list from a downstream legal-software stack kept
two things visible that veilq should have filled. A surname alone in the
column of opposing parties stayed: `name-field` needs two words, and NER
does not take one capitalised word without a first name for a name. The
addresses stayed too: the list keeps street, number, postcode and town in
columns of their own and cuts a long cell (`Hachenburg …`). `address`
needs a street word at the end of the street (`Lindenweg`) and
`address-postcode` a comma; a row of a table has neither, and a cut cell
has no street word. A synthetic CSV with the same columns lost the same
values: each cell is read alone.

In a picture, a line's context is the line on its left and the line above
it. In a table the line above is the row before; the column's head
(`Gegner`, `Straße`) is context for the first row only.

Reproducing it showed an older fault of the same kind: a candidate below
its threshold claims its span from every other. `name-field` takes a whole
value (`Termin Kerstin Volkmann`, `Herr Jonas Brandt`) at 0.35, and the
name NER or the salutation finds inside it was dropped as overlapped.

## Decision

1. **A cell in a picture reads the head of its column.** The column is the
   lines above it that overlap it and are aligned with it (left, middle or
   right); its head is the topmost of them, if that line stands in a row of
   three or more lines that head a column. The head is put after the lines
   beside it, on a line of its own. A line in such a row of heads has no
   label: its left neighbour is another head.
2. **A rule at confidence 0 finds with its context only,** and reads only
   the context's last line: a column's or key's name, and in a picture the
   head of the column. A rule at 0 without its context is no candidate,
   since a candidate below the threshold still claims its span.
3. **One word under a column that names a person is a name**
   (`intl.name-column`, 0 + 0.85): `Nachname`, `Gegner`, `Schuldnerin`,
   `Mandant`, … `name-field` takes the role words too (`Gegner`,
   `Schuldner`, `Gläubiger`, `Kläger`, `Beklagte`, `Beteiligte`). A column's
   head is no name in its own column: those words are role words
   (`NOT_NAMES`), except `Gläubiger`, which is a surname as well.
4. **A whole value under a column that names a street is an address**
   (`de.street-field`, 0 + 0.85): `Straße`, `Strasse`, `Street`,
   `Anschrift`, `Adresse`, also cut (`Hachenburg …`). Number, postcode and
   town alone stay: without the street they name no one's home.
5. **A row with street, number, postcode and town in cells of their own is
   an address** (`de.address-columns`, 0.85), as a picture's row joins its
   cells with two spaces, also when the street cell is cut or OCR reads
   the dots as a cell of their own. This finds without a head, which a
   screen may cut as well.
6. **A street word may be a word of its own** in `address` and `street`
   (`Trebuser Str. 3d 15517 Fürstenwalde`, `Berliner Straße 12`).
7. **A candidate below its threshold claims its span from other types
   only.** One of its own type at or above the threshold wins over it: a
   name inside a whole value is found. An IBAN with a wrong checksum still
   keeps a phone number out of its digits.

## Alternatives considered

- **Join a row's cells into one address in CSV and XLSX:** the engine reads
  a cell at a time; a join across cells would change how every format is
  read and how a replacement is put back. The street column alone carries
  what makes an address personal.
- **Fill postcode and town too:** a town is no one's address once the
  street is gone, and a list of towns would lose its use.
- **One word anywhere as a name:** every capitalised noun in German.
- **The head as part of the line's context for every rule only:** a form's
  label column looks like a column too, and the label above a label
  (`Anschrift` above `Telefon`) made `Telefon` an address in the corpus.
  Reading the head alone, and only of a row of three or more heads, keeps
  a form's labels out.
- **Drop a candidate below the threshold before overlaps:** an IBAN with
  a wrong checksum would no longer keep a phone number out of it
  (ADR-0012).

## Consequences

- A list of surnames, streets cut short and addresses in columns is
  filled, in pictures, CSV, XLSX and JSON (where the key is the column).
- A two-column table (`Name | Ort`) has no row of three heads: a surname
  alone there stays to NER. So does a surname under a head the screen cut
  (`e …`).
- A single word under a person's column that is no name (`Unbekannt`) is
  filled; fail closed (AGENTS.md, point 7) takes the false positive. So is
  a row of a stock list shaped like an address (`Schrauben  12  10000
  Lager`).
- A policy may write a rule at 0; it then finds with its context only, and
  reads its last line.

## Enforced by

- `crates/veilq-core/src/visual.rs` (`neighbours`, `in_head_row`, `heads`,
  `column`), test `a_cell_of_a_table_reads_the_head_of_its_column`
- `crates/veilq-core/src/detect/mod.rs` (`candidates`: a rule at 0;
  `decide`: overlaps; `NOT_NAMES`), tests
  `a_name_below_its_threshold_does_not_hide_one_above`,
  `a_column_of_a_list_names_a_person_or_a_street`,
  `an_address_in_the_cells_of_a_row_is_one`
- `crates/veilq-core/locales/intl/patterns.toml` (`name-column`,
  `name-field`), `crates/veilq-core/locales/de/patterns.toml`
  (`address`, `street`, `address-columns`, `street-field`)

## What this ADR does not say

That a table is understood: veilq still reads texts, and a column is known
only by its head's words. Which other columns name personal data (a
birth date, a phone number) is left to the patterns that already read a
column's name.
