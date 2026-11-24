# ADR-0069: Points from the review before 0.8.0

- **Status:** Accepted (the owner chose the points to fix before 0.8.0 in the conversation, 2026-11-24; the others are issues)
- **Date:** 2026-11-24
- **Deciders:** project owner
- **Related:** [ADR-0061](0061-a-name-broken-across-lines-kin-words-and-saints.md), [ADR-0062](0062-points-from-the-review-before-0-7-0.md), [ADR-0063](0063-what-a-document-says-besides-its-text-is-read-removed-or-reported.md), #156, #157, #158

## Context

The code and security review of `v0.7.0..HEAD` (documented internally)
found ten points. Six let personal data through:

1. A file or folder name's last dot: up to five letters after it were kept
   as an extension, unread (`Akte_Lena.Vogt`), in a folder run and in a
   mail attachment's name.
2. A mail header's parameters were split at every `;`, also inside a
   quoted file name (`filename="Akte; Frau Tilda Brenner.txt"`): the name
   was read in part, nothing found, and the header written out as it was.
3. NER left out a word over 64 characters whole, also one joined by `_` or
   `-` (`kunde_Kai_Lindqvist_vertrag_…`), without an error or a reason.
4. A self-closing `<script src="…"/>` in an HTML mail opened a script that
   no tag closed: every later comment was kept instead of removed.
5. A person span that starts like a saint (`Santa Cruz`, `St. Clair`,
   `Saint-Exupéry`) was dropped (ADR-0061), also after `Frau` or `Herrn`.
6. A kin word that is also a surname (`Vater`, `Tante`) made NER's span
   `Herrn Vater` no name (ADR-0061).

Two others break the output (a table column's name in xlsx, #156; a pivot
cache's types, #157), one costs memory (#158), and one is the run time of
the tiles of ADR-0068, decided there.

## Decision

1. **The extension is read with the name:** the name's stem and what
   follows its last dot are read together; is something found after the
   dot, the name has no extension, otherwise it keeps it.
2. **Header parameters are split outside quotes** (`split_params`), for
   reading and for writing the new name.
3. **A long joined word is read in its pieces:** a word over 64 characters
   joined by `_` or `-` is read as its pieces; only a piece over 64 is left
   out, and no name is that long.
4. **A self-closing `style` or `script` opens nothing.**
5. **A saint is no saint after a salutation or joined by a hyphen:**
   `Frau Santa Cruz`, `Herrn Saint-Exupéry` stay persons; `Sankt Martin`
   in running text stays none.
6. **A kin word after a salutation is a surname** for NER's spans
   (`Herrn Vater`); a headline's `kann Vater nicht vergeben` stays none.

Bug classes: `silent-fallback` (3: a word left out without a reason),
`naive-split` (2: a separator inside quotes), `unclosed-state` (4), and
`over-broad-rule` (1, 5, 6: a rule meant for one case dropped names). The
siblings searched: every `split(';')` of a header (three, all changed);
`bounded()`, which drops a single word over 1,024 tokens, unreachable with
the 64-character bound (left); the lists of `only_role_words`,
`ACCOUNTS`, `PRODUCTS` and `brand_word`, which drop a span whatever the
words before it (left: each is a whole word, not a prefix).

## Consequences

- Each point has a unit test that failed before its fix. The corpus, the
  held-out seeds, redacting and the handwriting gate (with the held-out
  seeds) are unchanged.
- A name after `Frau` or `Herr` that starts with `Sankt` or `St.` is filled
  now, also a saint's day after a salutation, which is rare.
- #156, #157 and #158 stay open for a later release.
