# ADR-0023: Pictures are read with OCR and redacted with black boxes

- **Status:** Accepted (card M4, points 1 to 7, approved 2026-11-10); amends [ADR-0005](0005-models-are-fetched-explicitly-and-pinned-in-the-binary.md) and [ADR-0019](0019-office-files-are-edited-in-place-every-part-read.md); amended by [ADR-0026](0026-a-picture-is-filled-until-a-round-finds-nothing-new.md) (rounds)
- **Date:** 2026-11-10
- **Deciders:** project owner
- **Related:** plan M4, [ADR-0022](0022-ner-is-gliner-in-onnx-optional-and-pinned.md), [ADR-0013](0013-a-synthetic-corpus-with-truth-and-golden-outputs-gates-recall.md)

## Context

Screenshots, photos of forms, screen recordings and scanned letters carry
the same personal data as text, as pixels. Until M4 veilq refused them
(ADR-0003), and blanked the images inside Office files and mails only when
a policy asked for it. Reading pixels takes a model, and a PDF takes a
renderer: both are code and data veilq loads, not code it ships.

## Decision

- **OCR:** PaddleOCR's detector PP-OCRv3 and its Latin recogniser PP-OCRv5
  (Apache-2.0), in the ONNX export `monkt/paddleocr-onnx` publishes on
  Hugging Face at revision `7b02d0a3`, pinned by SHA-256 and fetched only by
  `veilq models fetch ocr`, on the ONNX Runtime NER uses. Pre- and
  post-processing are veilq's own (DB regions, greedy CTC). A character's
  box comes from its CTC steps, one step wider on each side.
- **Detection on the read text:** the lines of a picture are one text in
  rows, and each line is read again with its left and upper neighbour as
  context (a form's label). Patterns and NER run on both; a finding becomes
  the boxes of its characters, two pixels wider, filled **black**. Verify
  reads the filled picture again, up to three rounds (ADR-0022; six since ADR-0026). Every
  action is a black box; when a policy asks for `pseudonymize` or `fake`
  the report says so as a review reason.
- **Metadata is always removed:** the output is encoded anew (JPEG at
  quality 90, PNG and WebP lossless), so EXIF, GPS, XMP and text chunks are
  gone; EXIF orientation is applied first. EXIF in the input is a
  `METADATA` finding over the whole picture.
- **GIF:** frame by frame. A box stays in the following frames while its
  pixels stand still; a frame equal to the one before is not read again;
  every frame that changed is read and verified.
- **PDF:** every page rendered at 200 dpi with PDFium (BSD-3-Clause, the
  `bblanchon/pdfium-binaries` build **chromium/8125**, the API
  `pdfium-render` 0.9.5 binds), pinned per platform as the component `pdf`
  and loaded at run time. The output is a **new PDF of page images**
  (JPEG), written by PDFium: text layer, hidden text, forms, annotations'
  contents, attachments, scripts, earlier revisions and metadata are gone,
  and the report says so (`review_required`).
- **Without OCR** a picture is an error, a PDF also without PDFium. Inside
  Office files and mails a PNG, JPEG, WebP or GIF (and a PDF attachment)
  is redacted like a file when OCR is installed; otherwise
  `[office] embedded` decides as before. A region names its `part` (an
  optional field in the report schema) and its page or frame.
- **Corpus and UI:** the generator draws a screenshot, a form photo with
  EXIF, a screen recording and a scanned letter with Noto Sans (OFL 1.1,
  in `crates/veilq-eval/assets`); a value's truth is the box of its ink.
  `veilq-eval check --ocr` counts it covered when the boxes found cover
  98 % of it. The UI sets a still image beside its output, each finding a
  frame on both sides.

## Alternatives considered

- **Blur or pixelate:** lighter on the eye, but parts can be recovered;
  black cannot.
- **Fill whole lines:** simpler, but takes labels and context away.
- **Draw the replacement text:** fonts, sizes and space do not fit; a
  wrong stand-in looks real.
- **Edit a PDF's text layer in place:** incremental updates, embedded fonts
  and hidden text leave remains that are hard to prove gone.
- **`lopdf` to write the PDF (on the card):** PDFium is loaded anyway and
  writes the image PDF; one dependency fewer.
- **PDFium chromium/8138 (on the card, the newest build):** its API is
  newer than the one `pdfium-render` binds; 8125 matches it.
- **A server for pictures:** sends unredacted data off the machine; kept
  for video and audio in M5 (`veilq serve`, optional).

## Consequences

- OCR adds about 10 MB of models and PDFium a few MB; a page takes a few
  seconds on a CPU.
- A redacted PDF cannot be searched or copied from; its size grows with
  its pages.
- Text OCR cannot read (handwriting, low contrast, rotated text, scripts
  other than Latin) is not found. The detector v5 has not been measured
  against v3.
- The corpus needs OCR to measure pictures, so CI measures them only when
  the models are installed; the golden tests skip them.

## Enforced by

- `crates/veilq-core/src/ocr.rs`, `visual.rs`, `pdf.rs`, `runtime.rs`;
  pins in `crates/veilq-core/src/models.rs`
- `crates/veilq-core/src/office.rs`, `mail.rs` (pictures inside)
- `crates/veilq-eval/src/generate/visual.rs`, `check.rs` (`COVER`)
- `crates/veilq/src/ui.rs`, `ui/src/components/ImagePane.vue`

## What this ADR does not say

Nothing about video or audio (M5), nor about reading handwriting.
