# ADR-0038: Faces in pictures are found and filled black

- **Status:** Accepted (card in the conversation, points 1 to 6, approved by the owner 2026-11-12); amends [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md) and [ADR-0005](0005-models-are-fetched-explicitly-and-pinned-in-the-binary.md)
- **Date:** 2026-11-12
- **Deciders:** project owner
- **Related:** [ADR-0022](0022-ner-is-gliner-in-onnx-optional-and-pinned.md), [ADR-0030](0030-a-gif-frame-is-read-where-it-changed.md)

## Context

ADR-0023 redacts the text of a picture. A screenshot of a news page, a
photo of a team or a scanned ID card also shows faces, and a face is
personal data no OCR reads: a redacted screenshot of a news feed came out
with every name filled and every portrait left. Whoever reads "redacted"
expects the faces gone too.

The face detectors that run on a CPU in ONNX and whose weights may be used
commercially are few: insightface's RetinaFace and SCRFD carry weights for
non-commercial research only, the YOLO face models are AGPL, MediaPipe's
BlazeFace (Apache-2.0) is made for a selfie at arm's length and misses the
small faces of a screenshot. YuNet (OpenCV Zoo, MIT, 230 KB) reads faces
from about 10 pixels up, but only at a fixed 640 x 640 input.

## Decision

- **Model:** YuNet 2023mar, pinned like every model (ADR-0005) as the
  component `face` (`face/yunet-2023mar/model.onnx`, from
  `opencv/face_detection_yunet` on Hugging Face at a fixed revision), run in
  the ONNX Runtime that NER and OCR load.
- **Found, never recognised:** veilq finds where a face is. It computes no
  features of a face, compares none, keeps none, and nothing leaves the
  machine. A face is a box, like a word.
- **`FACE` and a black box:** each face is a `FACE` finding (detector
  `face`, a region), its box grown by 15 % on every side (YuNet's box ends
  at brow and chin), filled black before OCR reads the picture. As with
  text, every action is a box; `pseudonymize` or `fake` say so in the
  report.
- **On by default, fail closed:** `[picture] faces = "redact"` is the
  default. Without the component, a picture (an image, a GIF, a PDF, an
  image inside an Office file or a mail) is refused as without OCR, and an
  embedded image falls to `[office] embedded`. `faces = "keep"` leaves
  faces, for pictures without people or whose people may be seen.
- **A low threshold, a review:** YuNet's score from 0.5 counts; it is
  reported as a confidence the default threshold (0.7) keeps (0.5 -> 0.7,
  1.0 -> 1.0), so a policy raises `FACE`'s `min_confidence` to fill fewer.
  A hand filled black costs less than a face left. A picture in which faces
  were found is `review_required`: one more, in profile or covered, may be
  left.
- **A pyramid of tiles:** a picture is read scaled to fit 640, then at
  twice that scale, up to its own size, each scale in tiles of 640 that
  overlap by 160; one face found at several scales or anchors is one. A GIF
  frame that changed is searched whole (a face cut by a band of ADR-0030 is
  still a face); a face inside a box kept from the frame before is that
  box's.
- **Synthetic faces only:** tests and the corpus never hold a photo of a
  real person. The detector's model test reads a picture named by
  environment variables, on the machine that runs it.

## Alternatives considered

- **Blur or pixelate the face:** parts can be recovered (ADR-0023).
- **Recognise who it is, to keep some faces:** that would be biometric
  processing (GDPR Art. 9) and needs a template of each face; veilq keeps
  nothing.
- **Off by default:** a face would leave silently, which ADR-0003 rules out.
- **BlazeFace, RetinaFace/SCRFD, YOLO-face:** see the context: too close,
  not usable commercially, AGPL.
- **One pass at 640:** a 2560-pixel screenshot scaled to 640 shrinks a
  40-pixel face to 10, at the edge of what YuNet reads.

## Consequences

- Every user who reads pictures fetches one more component (`veilq models
  fetch face`, 0.2 MB), or sets `faces = "keep"`; a policy that did not
  mention faces now refuses pictures until then.
- A face's box is wider than the face and can cover a caption below it; a
  drawn or rendered face (a game character, a cartoon) is filled when
  YuNet takes it for one. Both cost legibility, not privacy.
- Faces YuNet does not see stay: from behind, far turned, very small,
  heavily covered. The report's review reason says so.
- The search took 0.4 s for a 1570 x 1157 screenshot on a laptop CPU,
  loading the model included; more tiles for a larger picture. OCR still
  dominates.
- The synthetic corpus has no faces yet. Until a licence-clean set of
  generated faces is in it (its licence reviewed like a model's), the gate
  checks only that no face is found where there is none.

## Enforced by

- `crates/veilq-core/src/face.rs` (detector, pyramid, decoding), `visual.rs`
  (`faces`, before OCR, per changed GIF frame)
- `crates/veilq-core/src/models.rs` (`face` pinned), `policy.rs`
  (`[picture] faces`), `engine.rs` (`reads`: no picture without the
  detector unless faces are kept)
- `schema/policy.schema.json`; `.github/workflows/ci.yml` fetches `face`
  for the model gate

## What this ADR does not say

- Nothing about other marks of a person in a picture: a tattoo, a licence
  plate as a drawing, a person seen from behind.
- Nothing about video beyond GIF (M7).
