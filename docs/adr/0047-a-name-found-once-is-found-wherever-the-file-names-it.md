# ADR-0047: A name found once is found wherever the file names it

- **Status:** Accepted (card in the conversation, points A1 to A6, approved by the owner 2026-11-13); amends [ADR-0004](0004-the-policy-is-veilq-toml-in-the-current-directory-or-the-git-root.md) (a new section)
- **Date:** 2026-11-16
- **Deciders:** project owner
- **Related:** [ADR-0044](0044-a-deny-list-names-the-people-a-text-can-name.md), [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md), [ADR-0026](0026-a-picture-is-filled-until-a-round-finds-nothing-new.md), #34, #39

## Context

Each text veilq reads (a line of a picture, a cell, a chunk, a part of an
Office file) is decided on its own. A screenshot of a record showed the
person's name filled in the record's header and in the letter beside it,
and left in the application's tab (`Brandtmeier, Jon..`, cut short by the
tab) and in a sidebar the window cut on the left (`andtmeier, Jonas`):
there NER had nothing around the name to go by. The name was known; the
file named it again. A deny list (ADR-0044) finds a known name everywhere,
but only a name written into the policy beforehand.

## Decision

1. **A list of the file's names (A1):** each `PERSON` kept at or above its
   threshold, by any detector but this one, puts the words of its name on a
   list: three letters or more, no role word, no brand word, no system
   account, and no small word inside a capitalised name (`von` in `Ursula
   von der Leyen`). Every text read after that finds each word as a whole
   word, matched as a deny list's value is (ADR-0044): any case, Turkish
   `İ`/`ı` as one letter, NFC and NFD, with the genitive. A name new to the
   list is looked for in the text it was found in as well.
2. **One file with its parts (A2):** the list belongs to one run: a file
   with its pages, frames, Office parts, embedded images, a mail's parts
   and attachments. A folder's files do not share one; a list across them
   would tie the files together and depend on their order.
3. **The finding's confidence, detector `names:file` (A3):** a word found
   again is a `PERSON` at the highest confidence it was found at, detector
   `names:file`. A finding below its threshold puts nothing on the list. It
   is an ordinary candidate: a longer one claims its span, the allow lists
   name it away, a run threshold (ADR-0046) holds it like any other.
4. **Cut words in pictures (A4):** in text read by OCR, the start of a word
   of the list (three letters or more) with `..` or `…` after it
   (`Jon..`), and the end of one (five letters or more) at the start of a
   line (`andtmeier,`) are that word.
5. **On by default (A5):** `[names] everywhere = true`; `false` leaves each
   text to its own detectors.
6. **More found, a false one too (A6):** a word wrongly taken for a name is
   found wherever it stands. Role words and brands are filtered before the
   list, and filling a word too many is the side veilq errs on.
7. **Read again where the input is whole:** the list learns forward. Each
   text also notes its words outside its findings; a word learned after it
   already stood unfound in an earlier text makes the run late. An input
   held whole (every container: pictures, PDF, Office files, mails; and a
   text the MCP server's tools or the UI's editor hand over) is then read
   once more with the list known from the start, and that second reading
   is the run's result (its explanation replaces the first's). Late again
   after it, the report asks for a review.
8. **Streamed input asks for a review:** a streamed run (a text, NDJSON or
   CSV file from the command line or a folder in the UI, R7) writes what it
   read and holds constant memory; it cannot read its start again. A name
   learned after it stood unfound there is a review reason, not left
   silently. A plain text is read in chunks of about 64 KB, each as one
   text: within a chunk a name before its finding is found (point 1).

## Alternatives considered

- **Every file read twice:** the cost of a second OCR and NER reading on
  every picture and document, where most learn nothing late.
- **Confidence 1.0, as a deny list:** a false name would spread at full
  weight and claim spans from better candidates.
- **One list for a folder:** the result of a file would depend on which
  files were read before it.
- **Streamed input read whole:** breaks constant memory for a log (R7).

## Consequences

- A record's name is filled in its tabs, sidebars and lists, also where a
  window cut it.
- A file whose name stands before the place it is found is read twice
  when held whole: up to twice the time for such a file.
- A word NER took wrongly for a name is filled wherever it stands.
- A streamed run may ask for a review it did not ask for before.
- A shared string of a workbook is read under each column that shows it
  and without one; of the readings' findings the longest now wins, not the
  first: without a column the list finds a full name's words one by one,
  and under its column the name is one finding.

## Enforced by

- `crates/veilq-core/src/names.rs` (`Names`, `DETECTOR`), tests
  `a_name_found_once_is_found_again_in_every_form`,
  `role_words_brands_and_particles_stay_off_the_list`,
  `a_word_cut_by_a_windows_edge_is_found_in_a_picture`,
  `a_name_learned_after_it_stood_unfound_is_late`
- `crates/veilq-core/src/engine.rs` (`decided`, `whole`, `run`), tests
  `a_name_found_once_is_found_wherever_the_file_names_it`,
  `a_streamed_run_learns_names_forward_and_asks_for_a_review_behind`
- `crates/veilq-core/src/visual.rs`, test
  `a_name_in_a_tab_and_a_cut_sidebar_is_the_name_of_the_record`
- `crates/veilq-core/src/office.rs` (`Workbook::shared_strings`), the
  golden reports `eval/golden/de/xlsx/`
- `crates/veilq-core/src/policy.rs` (`Names`), `schema/policy.schema.json`
