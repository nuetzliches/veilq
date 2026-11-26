# ADR-0060: A line read badly is read again as handwriting

- **Status:** Accepted (card in the conversation, points 1 to 6, approved by the owner 2026-11-19); a first step of M5 ([#59](https://github.com/nuetzliches/veilq/issues/59), `docs/plan.md` 3.6); amended by [ADR-0072](0072-a-second-reading-as-handwriting-near-the-first-and-a-cut-line-over-ink.md) (a second reading counts near the first)
- **Date:** 2026-11-19
- **Deciders:** project owner
- **Related:** [ADR-0005](0005-models-are-fetched-explicitly-and-pinned-in-the-binary.md), [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md), [ADR-0045](0045-text-read-badly-is-reviewed-filled-or-stops-the-run.md), [ADR-0059](0059-a-files-values-firms-as-parties-and-lines-cut-by-the-edge.md)

## Context

PaddleOCR's Latin recogniser was trained on print. On a form filled in by
hand it misreads letters inside a line (`2oo95` for a postcode, a digit of
an IBAN as a letter), and a value misread is a value no detector finds. #52
made such a line a review reason where its letters score low; nothing read
it better. The plan (3.6) puts a reviewed transcript before the redaction;
this step brings the recogniser, without the transcript as a stage yet.

## Decision

1. **The recogniser (point 1):** `naeyn/de-htr-web-v2`, int8, 10.4 MB: a
   line-level CTC model (ResNet18 and three BiLSTM layers) for modern German
   handwriting, its alphabet with `ä ö ü ß`, 8 % character error rate on 78
   writers held out of its training. Weights Apache-2.0; trained on the
   ScaDS.AI German handwriting lines (CC BY 4.0) and
   `fhswf/german_handwriting` (AFL-3.0), as its `NOTICE-training-data.md`
   says. CTC gives each character its columns, so a finding fills its own
   words, not the whole line.
2. **The download (point 2):** a component of its own, `htr`, fetched only
   by `veilq models fetch htr` or the UI's setup, from its repository at
   revision `6241c1ff…`, the model and its `config.json` (the alphabet)
   checked against SHA-256 compiled into veilq, as OCR's models are. Not
   mirrored. Without it nothing changes.
3. **The second reading (point 3):** a line OCR read with a letter or
   digit below `WEAK` (#52) is read again by it, from the same box: grey,
   64 rows high, at most 1024 columns per piece. Both readings stay; each
   is searched as a line of its own with its neighbours as context, and
   what either finds is filled. The page, the text across lines, is built
   from the first readings only. The second reading is no review reason of
   its own under `low_confidence`; the line it reads again already is.
4. **The contract (point 4):** no change to the report's schema, the CLI's
   flags or the policy. A run that read a line as handwriting says so as a
   review reason, so `review_required` holds.
5. **Boxes to the end of the ink (decided while building):** a box filled
   over text grows up and down while its top or bottom row cuts ink (a
   pixel more than 60 off the box's most common colour in any channel), at
   most half its height each way, never past an empty row. A hand's loops
   and tails reached two or three rows past the box OCR found; those rows,
   left unfilled, showed part of the value. Printed text has background at
   its box's edges and keeps its box.
6. **The corpus (point 5):** forms filled in by hand, four per corpus,
   their values written in Indie Flower (OFL), in a track of their own,
   `eval/handwriting/`, with a gate of their own
   (`eval/gate-handwriting.toml`), read only with `--only handwriting`. The
   main corpus and its gates are unchanged.

Not in this step (point 6): the transcript as a stage of its own (review
1, `--transcript`), the three-part view in the UI, and cases from review 1.

## Alternatives considered

- **TrOCR:** its handwritten weights were fine-tuned on IAM, whose terms
  are research-only, and it reads English; it decodes token by token and
  gives no character's place, so a finding would fill the whole line.
- **Kraken models:** CC BY-SA or trained on historical hands.
- **A small vision language model:** gigabytes, no confidence, and it
  writes what is likely, not what is there.
- **The second reading in place of the first:** the two models' scores are
  not alike, and a printed line read worse by the hand's model would lose
  its findings.
- **A mirror under `nuetzliches`:** a publication of ours with nothing to
  gain over a pinned revision and checksum.
- **The forms in the main corpus:** a hand is read far worse than print;
  its misses would have lowered the main gate for every type.

## Consequences

- Measured on the handwriting track, seed 1 and the held-out seeds 7001 to
  7005 (120 values): 76 covered with the recogniser before the boxes grew
  to the ink, 94 with the boxes and without the recogniser, 101 with both.
  Every type but the address and the phone number is covered on three
  forms of four or more in every corpus; the gate is the worst corpus's,
  raised as veilq gets better.
- A line OCR reads badly costs one more model call when `htr` is
  installed.
- A box over text grows where its edge cuts ink, over a picture's frame or
  rule too where one touches it.
- What a hand writes that both readings miss stays; the review reason says
  so.

## Enforced by

- `crates/veilq-core/src/models.rs` (`htr`, `htr_files`)
- `crates/veilq-core/src/ocr.rs` (`Htr`, `decode`, `softmax`,
  `read_edges`), tests `logits_become_probabilities_per_step`,
  `a_ctc_path_gives_characters_with_their_columns`,
  `the_handwriting_recogniser_reads_a_line` (with the model installed)
- `crates/veilq-core/src/visual.rs` (`HANDWRITING`, `to_ink`, `candidates`,
  `unread`), tests `a_value_read_only_as_handwriting_is_filled`,
  `a_box_that_cuts_ink_grows_until_the_ink_ends`
- `crates/veilq-eval/src/generate/visual.rs` (`handwritten_png`),
  `crates/veilq-eval/src/check.rs` (`HANDWRITING`),
  `eval/gate-handwriting.toml`, `.github/workflows/ci.yml` (`gate
  (handwriting)`)
