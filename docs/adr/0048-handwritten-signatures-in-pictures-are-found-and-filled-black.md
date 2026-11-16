# ADR-0048: Handwritten signatures in pictures are found and filled black

- **Status:** Accepted (card in the conversation, points B1 to B6, approved by the owner 2026-11-13; the model changed after measuring, approved 2026-11-16); amends [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md) and [ADR-0005](0005-models-are-fetched-explicitly-and-pinned-in-the-binary.md)
- **Date:** 2026-11-16
- **Deciders:** project owner
- **Related:** [ADR-0038](0038-faces-in-pictures-are-found-and-filled-black.md), #80

## Context

A screenshot of a letter or an invoice preview came back with every name
and address filled and the signature under them left. A handwritten
signature is no text, so no OCR line and no detector of text sees it; it
identifies its writer as a name does.

The signature detectors in ONNX are few. The YOLOv8 models derive from
Ultralytics and are AGPL. Apache-2.0 weights exist for a Conditional-DETR
with ResNet-50 (`tech4humans/conditional-detr-50-signature-detector`) and
for YOLOS base and tiny (`mdefrance/yolos-*-signature-detection`), all
fine-tuned on the same set (Tobacco800 and Roboflow `signatures-xc8up`).
Only YOLOS base had an ONNX file, of 511 MB (248 MB as int8).

Measured on 40 synthetic letters and screenshots (21 signatures, half of
them in screenshots with a letter beside a window), the four pictures of
the corpus and one reported screenshot with a real signature, on 4 CPU
threads, at score 0.5:

| Model | Size | Per screenshot, whole / squares | Found | False | Real signature |
|---|---|---|---|---|---|
| Conditional-DETR R50 | 177 MB | 163 / 905 ms | 16 / 20 of 21 | 0 | missed (one box over half the page) |
| YOLOS-tiny | 26 MB | 131 / 523 ms | 19 / 20 of 21 | 1 / 2 | 0.95, a tight box |
| YOLOS-tiny int8 | 9 MB | 120 / 477 ms | 20 / 21 | 1 / 6 | 0.92 |
| YOLOS-base int8 | 248 MB | 3.2 / 12.6 s | 17 / 20 | 0 | 0.95 |

Every false box of YOLOS-tiny fp32 lay on a large diagonal watermark
(`RECHNUNGS-ENTWURF`); none on the corpus.

## Decision

- **Model:** YOLOS-tiny (`mdefrance/yolos-tiny-signature-detection`,
  Apache-2.0) at a fixed revision, converted to ONNX with its weights
  unchanged by `scripts/export-signature-model.py` (reproducible: the same
  versions give the same file), published as
  `nuetzliches/yolos-tiny-signature-detection-onnx` with the base model's
  name, origin, licence and the note that only the format changed, and
  pinned by its SHA-256 as the component `signature`
  (`signature/yolos-tiny-signature/model.onnx`), run in the ONNX Runtime
  that NER, OCR and the face detector load. The first model veilq fetches
  from nuetzliches rather than from its maker: the maker publishes no ONNX.
- **Found, never compared:** veilq finds where a signature is. It computes
  no features of it, compares none, keeps none, and nothing leaves the
  machine.
- **`SIGNATURE` and a black box:** each signature is a `SIGNATURE` finding
  (detector `signature`, a region), its box grown by 5 % on every side,
  filled black before OCR reads the picture, after the faces. As with
  faces, every action is a box.
- **On by default, fail closed:** `[picture] signatures = "redact"` is the
  default. Without the component a picture is refused as without OCR, an
  embedded image falls to `[office] embedded`. `signatures = "keep"` leaves
  signatures.
- **A low threshold, a review:** a score from 0.5 counts and is reported as
  a confidence the default threshold keeps (0.5 -> 0.7, 1.0 -> 1.0). A
  picture in which a signature was found is `review_required`: one may be
  missed, and a watermark or a stamp may be taken for one.
- **Whole and in squares:** the model reads 640 x 640, the picture resized
  to it whatever its shape. A picture much wider or taller than square
  (from 1.3 : 1: a screenshot, an A4 page) is also read in squares of its
  shorter side that overlap by half. A box narrower than 40 pixels is
  dropped first: the model gate's screen recording had two such boxes on
  empty space, and no signature that narrow can be read. Of the rest, a box
  that lies (nearly) inside another is that one's and the outer box stays;
  boxes that only overlap are both filled. So a box of part of the strokes
  never takes the place of the whole, and no box grows over what lies
  between two (found in the review before 0.4.0).
- **Synthetic signatures only:** tests and the corpus hold handwriting-style
  fonts and drawn strokes, never a real person's signature. The model test
  reads a picture named by environment variables, on the machine that runs
  it.

## Alternatives considered

- **Conditional-DETR R50 (the card's first choice):** missed the reported
  signature and is seven times larger.
- **YOLOS-base:** as good on the real signature, but 248 MB as int8 and 25
  times slower.
- **YOLOS-tiny int8:** 17 MB smaller, but six false boxes in squares
  against two.
- **YOLOv8 signature models:** AGPL.
- **A heuristic of strokes without text:** takes logos, stamps and
  underlines for signatures and misses a signature that crosses text.
- **Only `review_required` without the component:** a signature would leave
  while the report only asks for a look; ADR-0003 rules that out.

## Consequences

- Every user who reads pictures fetches one more component (26 MB), or sets
  `signatures = "keep"`; a policy that did not mention signatures refuses
  pictures until then.
- Large diagonal watermark text is filled as a signature and can cover a
  table below it; #80 fine-tunes the model with watermarks as background.
- A signature's box takes a line below it as well when the strokes reach
  into it (the signer's printed name, a role).
- The search adds about 0.1 s for a page and 0.5 s for a wide screenshot on
  a laptop CPU; OCR still dominates.
- The weights were trained on Tobacco800 and `signatures-xc8up`; their
  terms are those of the base model's card.

## Enforced by

- `crates/veilq-core/src/signature.rs` (detector, squares, decoding),
  `visual.rs` (`signatures`, before OCR, per changed GIF frame)
- `crates/veilq-core/src/models.rs` (`signature` pinned), `policy.rs`
  (`[picture] signatures`), `engine.rs` (`reads`: no picture without the
  detector unless signatures are kept)
- `scripts/export-signature-model.py`; `schema/policy.schema.json`;
  `.github/workflows/ci.yml` fetches `signature` for the model gate

## What this ADR does not say

- Nothing about reading handwriting (#59) or verifying whose a signature is.
- Nothing about stamps, initials in a margin or a signature drawn in a
  form field of a PDF's text layer.
