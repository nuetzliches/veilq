# ADR-0062: Points from the review before 0.7.0

- **Status:** Accepted (card in the conversation, points 1 to 5, approved by the owner 2026-11-20; point 4 changed with the owner's approval, see there)
- **Date:** 2026-11-20
- **Deciders:** project owner
- **Related:** [ADR-0022](0022-ner-is-gliner-in-onnx-optional-and-pinned.md), [ADR-0047](0047-a-name-found-once-is-found-wherever-the-file-names-it.md), [ADR-0059](0059-a-files-values-firms-as-parties-and-lines-cut-by-the-edge.md), #138

## Context

The review of 0.7.0 left five points (#138). None let personal data
through in the corpus; they traded recall against false positives, or cost
run time and memory:

- Since #130 a word one edit off a learned name must start upper-case to
  be found in OCR text, so `rlchter` beside `Richter` is not found unless
  NER finds it; the rule keeps `richtet` from being filled.
- A firm's words that end like a trade (`bau`, `dienst`) are not learned
  (ADR-0059), and a sole trader's surname ending so (`Fernbau` in
  `Ilka Fernbau e.K.`) was not learned either.
- NER read one word of 16,000 characters as it stands; ONNX Runtime
  allocated tens of gigabytes before it failed, and the machine swapped.
- A GIF's declared screen was bounded, not its frames: each frame is
  decoded on the whole screen, so many tiny frames on a large screen took
  as long as frames times screen.
- A band of a GIF frame no longer looks for lines cut at its open edges
  (#129); a line taller than the margin around a changed row might leave a
  fragment outside the band.

## Decision

1. **A name read in lower case (point 1):** a word OCR read in lower case
   one edit off a learned name's word is not filled; the run asks for a
   review with the reason that a word read off a picture in lower case is
   one letter off a name of the file. Written text, a word of the list
   itself and `[names] similar = false` ask for nothing.
2. **A GIF's frames (point 2):** the frames of a GIF, each counted with its
   whole screen, may come to 20 Gpx (some 10,000 frames at 1920 x 1080,
   2,400 at 4K); past that the run stops with an error that names the
   screen and the frames (fail closed).
3. **A word too long for a name (point 3):** NER does not read a word of
   more than 64 characters; the patterns still see it. A window of more
   than 1024 tokens, its prompt included, is read in halves, and a single
   word over the bound is left out. Below the bound nothing changes.
4. **A sole trader's surname (point 4):** in a firm whose legal form is a
   sole trader's (`e.K.`, `eK`, `e. Kfm.`, `e. Kfr.`), a word that ends like
   a trade is learned when another of the firm's name words stands before
   it (`Ilka Fernbau e.K.`). A firm of the trade word alone
   (`Holzbau e.K.`) and every other legal form keep the filter. The card
   said "a firm whose name starts with a given name"; veilq has no list of
   given names, and the owner chose this rule instead. `Kfm` and `Kfr` are
   legal forms now and never go on the list themselves.
5. **A tall line at a band's open edge (point 5):** no change. A fragment
   outside a band is a row that stood still against the frame before; that
   frame showed the same pixels, filled where its reading found the value.
   A line the detector finds touching an open edge makes the band grow
   (`settle`). Only a value the frame before missed and the detector
   misses at the edge again could stay, and then the frame before shows
   it already.

## Alternatives considered

- **Filling a lowercase word at a lower confidence:** below the default
  threshold it is filled nowhere, and a lower threshold fills `richtet`.
- **Bounding the screen times frames by memory:** frames are decoded one
  at a time; memory is bounded already, time is not.
- **Bounding NER's tokens per word only:** 200 words of 64 characters each
  still make several thousand tokens; the window bound holds both.
- **Short trade endings as whole words only:** `Holzbau GmbH` would put
  `Holzbau` on the list of every file it stands in.

## Consequences

- A picture holding a word like `richtet` beside a learned `Richter` asks
  for a review; it is not filled.
- A recording of more than 20 Gpx is an error; it has to be cut first.
- A name beside a word of more than 64 characters is read without it; a
  dense window may lose context at the cut between its halves.
- `Nord Holzbau e.K.` puts `Holzbau` on the list (a false positive, no
  leak).

## Enforced by

- `crates/veilq-core/src/names.rs` (`candidates`, `doubtful`, `read`,
  `sole_trader`), tests
  `a_name_read_in_lower_case_one_letter_off_asks_for_a_review`,
  `a_sole_traders_surname_ending_like_a_trade_is_learned`
- `crates/veilq-core/src/engine.rs` (`whole_names`, `NAMES_DOUBTFUL`), test
  `a_name_read_in_lower_case_one_letter_off_asks_for_a_review`
- `crates/veilq-core/src/ner.rs` (`read_words`, `bounded`, `MAX_WORD`,
  `MAX_TOKENS`), tests `a_word_too_long_for_a_name_is_not_read`,
  `a_window_of_too_many_tokens_is_halved`,
  `the_model_finds_names_without_a_marking_word`
- `crates/veilq-core/src/visual.rs` (`gif_frames`, `MAX_GIF_PIXELS`), test
  `a_gif_of_more_frames_than_its_screen_allows_is_an_error`
