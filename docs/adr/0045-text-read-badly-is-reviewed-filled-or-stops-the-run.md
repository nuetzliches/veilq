# ADR-0045: Text read badly is reviewed, filled or stops the run

- **Status:** Accepted (card in issue #52, points C1 to C4, approved by the owner 2026-11-13); amends [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md) and [ADR-0040](0040-purposes-are-built-in-policy-packs.md)
- **Date:** 2026-11-13
- **Deciders:** project owner
- **Related:** [ADR-0026](0026-a-picture-is-filled-until-a-round-finds-nothing-new.md), #52, #58

## Context

The OCR reads printed text; handwriting, very small or blurred text comes
back with letters read badly, and a name in it may be read as something
else and not found. Since #58 a line with a letter or digit below 0.7 that
no box covers makes the report say `review_required` and where. A person
then looks. For a firm whose scans carry handwritten notes, a review per
picture is not enough: it wants such a line filled, or no output at all.

## Decision

1. **`[picture] low_confidence`** in the policy: `review` (the default,
   the behaviour since #58), `box` or `error`. An option, not set unless
   written, so a purpose can set it where the policy does not.
2. **`box`** fills every line not yet filled that holds a weak letter or
   digit, whole, and reports it as a finding of the new type `UNREAD_TEXT`
   (detector `ocr:low-confidence`, confidence 1.0), on the first reading of
   a picture, a GIF band or a PDF page, as the review reason is.
3. **`error`** stops the run at the first such line with an error that
   names its place (`Error::Unread`); nothing is written.
4. **Purposes:** a pack may set `[picture] low_confidence`; between packs
   the stricter wins (`review` < `box` < `error`), and the policy's own
   value comes first (ADR-0040). `legal` sets `box`: a handwritten name in
   a scanned file is a party as much as a printed one. The UI's purpose
   picker says so.
5. The threshold of a weak letter stays 0.7, as measured in #58; it is not
   a setting.

## Alternatives considered

- **A section `[ocr]` of its own:** `[picture]` already governs images,
  GIF frames and PDF pages, and faces.
- **`box` without a finding:** the report would not say why a line is
  black; a finding names the type and the place, never the value.
- **`legal` keeps `review`:** a review per picture is what a firm with many
  scans skips; a box loses a line of text, not a party.

## Consequences

- A picture with handwriting comes out with that line black under `box`;
  printed text read badly (a stamp, a faint copy) is filled too.
- A run under `legal` fills such lines where it reported them before;
  a policy that names `legal` and wants the review writes
  `[picture] low_confidence = "review"`.
- The report schema is unchanged (an entity type is a string);
  `UNREAD_TEXT` is a new value in it.

## Enforced by

- `crates/veilq-core/src/policy.rs` (`Picture::low_confidence`,
  `LowConfidence`, `Policy::resolved`), `crates/veilq-core/src/purpose.rs`
  (`PurposePicture`, `merge`), `crates/veilq-core/purposes/legal.toml`
- `crates/veilq-core/src/visual.rs` (`unread`, `UNREAD_TEXT`),
  `crates/veilq-core/src/error.rs` (`Error::Unread`), test
  `text_read_badly_is_reviewed_filled_or_an_error_as_the_policy_says`
- `schema/policy.schema.json`
