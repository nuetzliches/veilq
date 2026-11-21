# ADR-0065: A weak face box is shown to a verifier

- **Status:** Accepted (card in the conversation, points 1 to 5, approved by the owner 2026-11-21); amends [ADR-0038](0038-faces-in-pictures-are-found-and-filled-black.md)
- **Date:** 2026-11-21
- **Deciders:** project owner
- **Related:** [ADR-0005](0005-models-are-fetched-explicitly-and-pinned-in-the-binary.md), [ADR-0048](0048-handwritten-signatures-in-pictures-are-found-and-filled-black.md)

## Context

YuNet fills every face it finds (ADR-0038), also with a weak score: on a
screenshot of a news feed it boxed an app's icon (an asterisk) and a thumb
at an automated teller, both at a score just above its threshold, and four
drawn figures on a book's cover. A higher threshold was no way out: real
faces in a group photo scored as low.

## Decision

1. **The model (point 1):** `veilq-face-verify-v1`, 6 MB, a MobileNetV3-small
   (`timm/mobilenetv3_small_100.lamb_in1k`, Apache-2.0; pretrained on
   ImageNet, whose own terms of use remain a note) fine-tuned by nuetzliches
   on synthetic pictures only: faces of invented people generated with
   FLUX.1-schnell (Apache-2.0) and rendered from MakeHuman assets (CC0),
   drawn faces, icons, logos, smileys, hands, UI, and details of CC0 photos
   reviewed to hold no person. Three classes: a photographed face, a drawn
   face, no face. It tells nothing of whose a face is.
2. **Published and fetched (point 2):** on Hugging Face as
   `nuetzliches/veilq-face-verify-v1` with a model card, at a fixed revision
   and SHA-256; an optional component `face-verify` (`veilq models fetch
   face-verify`, the UI's setup). Without it nothing changes: every box
   YuNet finds is filled.
3. **Asked about weak boxes only (point 3):** a box whose YuNet score is
   below 0.8 is cut as veilq would fill it (grown by 15 %), and left
   unfilled only when the verifier gives `no face` a probability of at least
   0.9532 (its validation's threshold). A drawn face is a face and stays
   filled; smileys, emoji, icons, logos and hands are no face.
4. **The report says so (point 4):** a picture with a box left this way
   gets the review reason `FACES_LEFT`; schema, CLI and policy are
   unchanged.
5. **Measured (point 5):** see Consequences. A picture of the corpus with a
   synthetic no-face that YuNet takes for a face was not found: on procedural
   icons YuNet fires on none, on face-like drawings it fires on drawn faces,
   which the verifier keeps. A model test on two synthetic crops (an icon
   vetoed, a drawn face kept) and every picture gate of the corpus with the
   verifier installed stand in for it.

## Alternatives considered

- **Training YuNet again:** its training data (WIDER FACE) is for
  non-commercial research only.
- **A higher threshold for faces:** real faces in a group scored below the
  icon.
- **Every box to the verifier:** its second run left one large real face
  under a caption line unfilled (YuNet 0.95); the third keeps it, but a box
  YuNet is sure of gains nothing from a second opinion.
- **Drawn faces as no face:** a caricature can show a real person.

## Consequences

- On the news feed's screenshot (checked locally, never stored) the icon
  and the thumb are left, every real face (twelve in a group, one large
  under a caption line) and the drawn figures are filled.
- On its synthetic test: 99.95 % of the faces kept, 77 % of the no-faces
  left; among the boxes YuNet itself finds, no face scored above 0.42 for
  `no face`. Photo details are left least often (about half).
- A weak box the verifier wrongly holds for no face stays in sight; the
  review reason points at it.
- One more model call for each weak face box.

## Enforced by

- `crates/veilq-core/src/models.rs` (`face-verify`, `face_verify_files`)
- `crates/veilq-core/src/face.rs` (`Verifier`, `Faces::verify`,
  `VERIFY_BELOW`, `NO_FACE`), tests `logits_become_probabilities`,
  `the_verifier_tells_an_icon_from_a_face` (with the model installed)
- `crates/veilq-core/src/visual.rs` (`faces`, `FACES_LEFT`)
- `.github/workflows/ci.yml`: the picture gates run with the verifier
