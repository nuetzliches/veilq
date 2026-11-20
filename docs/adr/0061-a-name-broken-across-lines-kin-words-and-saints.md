# ADR-0061: A name broken across lines, kin words and saints

- **Status:** Accepted (card in the conversation, points 1 to 5, approved by the owner 2026-11-20)
- **Date:** 2026-11-20
- **Deciders:** project owner
- **Related:** [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md), [ADR-0047](0047-a-name-found-once-is-found-wherever-the-file-names-it.md), [ADR-0052](0052-a-possessive-and-a-noun-are-no-name.md)

## Context

A screenshot of a news feed, redacted with 0.7.0, showed three kinds of
error; each was rebuilt with a synthetic picture of three cards side by side:

- A headline broke after a person's given name; the surname started the
  line below. Its lines stood further apart (12 px at a box of 17 px) than
  the half line within which `below` puts lines into one cell, so the page
  set the cards side by side row by row. The given name was filled, the
  surname was left.
- NER called a kin word a person (`… kann Vater nicht vergeben`, 0.85),
  also in plain text, and the word went on the file's list (ADR-0047), as
  did `Zwillinge` from a twins' surname.
- NER called a book title's saint a person (`Die Geschichte von Sankt
  Martin`).

## Decision

1. **A name across a line break (point 1):** when a line read alone ends in
   a person's name (nothing but punctuation after it) and the line under it
   in its column starts with a capital, the two are read together as one
   text, as an address block is (`Am Anger 7` / `15938 Lindau`). Under
   means left aligned within a line's height, the same type size, and a gap
   of at most a line and a half: OCR's box holds the ink, so a headline set
   at 1.6 times its type size leaves more than a box's height between its
   boxes. Which words form the name the detectors decide on the joined text.
2. **Kin words (point 2):** `Vater`, `Mutter`, `Tochter`, `Schwester`,
   `Zwilling`, `Tante`, `Neffe`, `Nichte`, `Witwe`, `Witwer`, `Ehefrau` and
   `father`, `mother`, `daughter`, `sister`, `twin` (with their endings, at
   the end of a compound) are person nouns: NER's span of them alone is no
   name, and no such word goes on a file's list. `Sohn`, `Bruder`, `Oma`
   and `Opa` end common surnames (`Mendelssohn`, `Thoma`) and are left out.
3. **Saints (point 3):** a person's span that starts with `Sankt`, `St.`,
   `Saint`, `Santa` or `Heilige` is dropped as role words: a saint, a town,
   a church or a feast. An address with one is the address patterns'.
4. **No contract change (point 4):** the report's schema, the CLI and the
   policy stay as they are; `--explain` gives the reason `role_words`.
5. **Corpus (point 5):** a synthetic news feed, `eval/de/png/news-1.png`,
   with a name broken after its given name, a kin word and a saint's book.

## Alternatives considered

- **A wider gap for every cell:** `below` also joins the lines of a table's
  cells; a wider gap there would join rows of lists and tables.
- **The first word of the next line taken as a surname:** German writes
  every noun with a capital; `Morgen` after `… an Lena` would be filled.
- **A higher threshold for NER:** loses names that are found today.

## Consequences

- A surname at the start of a headline's second line is filled with its
  given name; a joined read costs one more reading per such line.
- A kin word is not filled alone, also where it is a name in a text (rare
  for the words chosen); a person beside it still is.
- A saint's name (`Sankt Martin`) is not filled, nor a person whose given
  name is written `St.`.

## Enforced by

- `crates/veilq-core/src/visual.rs` (`candidates`, `under`), test
  `a_name_broken_after_its_given_name_is_read_with_the_line_below`
- `crates/veilq-core/src/detect/mod.rs` (`PERSON_NOUNS`, `saint`), test
  `a_saint_and_a_kin_word_are_no_person`
- `crates/veilq-core/src/names.rs` (`learnable`), test
  `role_words_brands_and_particles_stay_off_the_list`
- `crates/veilq-eval/src/generate/visual.rs` (`news_png`),
  `eval/de/png/news-1.png.truth.json`
