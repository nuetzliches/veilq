# ADR-0068: A wide picture is read in tiles for signatures

- **Status:** Accepted (card in the conversation, points approved by the owner 2026-11-23; its amendment for veilq-signature-v5 approved 2026-11-24); amends [ADR-0048](0048-handwritten-signatures-in-pictures-are-found-and-filled-black.md)
- **Date:** 2026-11-24
- **Deciders:** project owner
- **Related:** [ADR-0005](0005-models-are-fetched-explicitly-and-pinned-in-the-binary.md), [ADR-0053](0053-every-detector-reads-the-picture-before-anything-is-filled.md)

## Context

veilq-signature-v4 found about two in three signatures on the small photo
cards of a web page (84 of 130 on its validation set). Of those it missed,
30 got no box at all: on a 2400 x 1350 page read whole and in squares of
its shorter side, a 60-pixel signature reaches the model at 16 to 28
pixels, and below about 20 it was rarely found. Better pictures alone did
not change that (a training with legible cards and other scribbles, no
better than v4). Reading wide pages in 640-pixel tiles with v4 found 96 of
130, but with 493 false boxes instead of 293: v4 had never seen a picture
at that scale.

## Decision

The owner approved the card on **2026-11-23** (a model trained on tiles
too, a wide picture read in tiles, a gate, the run time, the contract) and
its amendment on **2026-11-24**:

| # | Category | Point | Beats | Reversing costs |
|---|---|---|---|---|
| 1 | model | veilq-signature-v5: YOLOS-tiny fine-tuned again, on v4's pictures and on 640-pixel tiles of the wide ones; published as `nuetzliches/veilq-signature-v5`, pinned by revision and SHA-256 | v4: 109 of 164 web signatures with 322 false boxes on a fresh set | Pin v4 again |
| 2 | threshold | A box counts from score 0.65 (`MIN_SCORE`, was 0.5); 0.65 still becomes confidence 0.7 | v5 at 0.5: 304 false boxes instead of 203 on the fresh set | One constant |
| 3 | reading | A landscape picture from 1.3 : 1 and wider than 1280 pixels is also read in 640-pixel tiles of the original, half a tile apart; a box a tile found counts from 0.95; a portrait page is read as before | A signature that reaches the model below 20 pixels | Remove the tiles |
| 4 | gate | Measured on a set not used for choosing the thresholds (synthetic, seed 3): web recall at least 0.75, no kind of picture with more false boxes than v4; the corpus gates unchanged | The first card's 0.8 on the set the thresholds were chosen on | None, a measurement |
| 5 | contract | No change to the schema, CLI or policy; the model is pinned at a new path, so pictures are refused until `veilq models fetch signature` | – | – |

As built (`signature.rs`, `tiles`): the tiles are `640 x min(640, h)` of
the original, the last of each row and column at the far edge; each is
resized to 640 x 640 like every other part. Boxes from every part go
through the 40-pixel width floor and the merge of ADR-0048 together.

Decided while building, not on the card:

- **No confirmation rule.** The first card let a tile's box count below
  0.95 when a box of the whole or a square covered half of it. Measured on
  both sets it found no signature more and put two and three false boxes
  more; a tile's box counts from 0.95 only.

## Measured

800 synthetic pictures of a seed used neither for training nor for the
thresholds (529 signatures); v4 read as before (whole and squares, 0.5),
v5 as above:

| Kind | Found v4 / v5 | False boxes v4 / v5 |
|---|---|---|
| Letter without a watermark | 74 / 75 of 75 | 22 / 18 |
| Letter with a watermark | 115 / 117 of 117 | 19 / 13 |
| Letter photographed | 24 / 24 of 24 | 12 / 10 |
| Photo signed on its face | 62 / 60 of 70 | 18 / 6 |
| Web page with signed photo cards | 109 / 127 of 164 | 177 / 97 |
| Photo without a signature | – | 12 / 5 |
| Form filled in by hand | 78 / 77 of 79 | 52 / 48 |
| Printed text | – | 10 / 6 |
| All | 462 / 480 of 529 | 322 / 203 |

On the set the thresholds were chosen on (v4's validation set): 484 of 523
against 466, 220 false boxes against 293, web 104 of 130, forms 66 false
boxes (65), photographed letters 7 (5).

veilq's corpus with v5 against `main` with v4: identical, also on the
five held-out seeds and when redacting; the handwriting gate passes.

## Alternatives considered

- **Tiles with v4:** 96 of 130 web signatures, 493 false boxes.
- **Training on better pictures only:** no better than v4.
- **More tiles without a signature, false boxes weighed twice:** a training
  that chose an epoch finding little (web 68 of 130); no epoch passed.
- **A score of 0.5 with v5:** a third more false boxes than v4.

## Consequences

- A wide screenshot or web page takes 3 to 6 seconds longer on a laptop
  CPU (28 tiles of 2560 x 1350, about 0.2 s each): a scan of such a
  picture took 15 to 25 s with v4, OCR still the most of it.
- Two signatures on photos and one on a form that v4 found, v5 missed on
  the fresh set; about one in four signatures on small photo cards is
  still missed and stays visible. The picture stays `review_required`.
- An installed v4 does not count: `veilq models fetch signature`.
