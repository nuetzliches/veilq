# ADR-0050: A redacted picture names veilq and its version

- **Status:** Accepted (card in the conversation, points 1 to 6, approved by the owner 2026-11-17); amends [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md)
- **Date:** 2026-11-17
- **Deciders:** project owner
- **Related:** [ADR-0026](0026-a-picture-is-filled-until-a-round-finds-nothing-new.md), [ADR-0048](0048-handwritten-signatures-in-pictures-are-found-and-filled-black.md)

## Context

A redacted picture looked like any other picture. Once it left the machine,
nobody could tell it had been through veilq, or through which version, short
of comparing it with the original. veilq writes no metadata into its output
(ADR-0023 removes it on purpose), so the picture itself has to say it.

The mark must not weaken the redaction. A picture is filled and read again
by OCR until a round finds nothing new (ADR-0026), and the signature model
may put a box on a watermark (ADR-0048). A mark drawn before those rounds
would be read every round and could be filled or cause a finding.

veilq core draws no text today and ships no font. The one font in the
repository (Noto Sans, OFL 1.1, in `veilq-eval`) is not on the licence
allow list of `deny.toml`.

## Review card

| # | Category | Point | Beats | Cost of reversing |
|---|---|---|---|---|
| 1 | term | Every picture veilq writes (PNG, JPEG, WebP, GIF frames, PDF pages) carries the mark `veilq <version>` at the bottom right, on by default. | No mark. | Low: flip the default. |
| 2 | contract | Opt-out is `[picture] watermark = false` in the policy (MCP and UI follow it) and `veilq redact --no-watermark` for one run. | A CLI-only switch. | Medium: a published key and flag need a deprecation. |
| 3 | trust | The mark is drawn after redaction and verification, onto the output only. | Drawing before the verify rounds. | Low. |
| 4 | concept | Only the file the caller hands in is marked; images inside Office files and mails, and text formats, stay unmarked. | Marking every embedded image. | Low. |
| 5 | concept | A built-in 5x7 bitmap font, light text on a dark translucent box, scaled to the picture; a picture too small for it gets none. | Shipping Noto Sans and `ab_glyph`. | Low. |
| 6 | concept | In a PDF the mark is pixels in the page image, not a text object. | A PDFium text object. | Low. |

Placement: an overlay at the bottom right, chosen by the owner over a strip
added below the picture (which would cover nothing but change its size).

## Decision

veilq draws `veilq <version>` (the build's `CARGO_PKG_VERSION`) at the
bottom right of every picture it writes: a PNG, JPEG or WebP image, each
frame of a GIF, each page of a PDF. The letters come from a 5x7 bitmap table
in `veilq-core` and are drawn light on a dark box at 59 % opacity. An image
gets one glyph pixel per 500 pixels of its shorter side (at least one), and
the box sits two glyph pixels from its edges. A PDF page, rendered at
200 dpi, gets glyphs 14 pixels (about 1.8 mm) high, a quarter inch from the
page's bottom and right edges. A picture whose box would take more than half
its width or a quarter of its height gets no mark.

The mark is drawn after `redact_picture` returns, so after the verify
rounds, and only when veilq writes output. OCR, the face and signature
detectors never see it, and it cannot hide or add a finding. A GIF frame is
marked as it is encoded; the next frame is compared with the redacted,
unmarked one.

The mark is on by default. `[picture] watermark = false` in the policy
switches it off for every run of that policy: the CLI, the MCP server and the
UI. `veilq redact --no-watermark` and the folder run's checkbox in the UI
switch it off for one run; they cannot switch on what the policy switched
off. Pictures inside DOCX, PPTX, XLSX and mails are left unmarked, and so is
every text format.

## Alternatives considered

- **No mark, or metadata instead:** metadata is stripped by design and lost
  on the first re-encode elsewhere; the owner asked for a visible mark.
- **Drawn before the verify rounds:** OCR would read the mark every round;
  the signature model can box a watermark (ADR-0048), and a name detector
  could take `veilq` for a word of a name.
- **Every embedded image marked:** a document with dozens of images would
  carry dozens of marks, inside pictures the reader may not see as separate.
- **A TrueType font with `ab_glyph`:** smoother letters, but a new dependency
  and a font licence (OFL) outside the allow list, for a dozen characters.
- **A text object in the PDF:** crisp at any zoom, but the PDF would gain a
  text layer again, after ADR-0023 made it image-only on purpose.
- **A strip added below the picture:** covers nothing, but changes the
  picture's size, which a caller placing it may rely on.

## Consequences

- Every redacted picture differs from before in its bottom right corner;
  the mark covers about 0.15 % of a 1920x1080 image. Whatever stood there in
  the original is partly hidden under a translucent box.
- veilq run again on its own output reads the mark with OCR. It holds no
  personal data; should a detector ever take it for one, the box over it is
  a false positive, not a leak.
- The mark says which version wrote a picture, which helps when a fixed
  detector is the question. It does not prove anything: it is pixels anyone
  can draw or remove.
- A version string with characters outside `0-9 a-z . - +` would show a box
  for each; the release process keeps to semantic versions.

## Enforced by

- `crates/veilq-core/src/watermark.rs` (glyphs, scale, drawing, tests)
- `crates/veilq-core/src/visual.rs` (`image`, `gif`), `crates/veilq-core/src/pdf.rs` (`redact`)
- `crates/veilq-core/src/office.rs`, `crates/veilq-core/src/mail.rs` (parts unmarked)
- `crates/veilq-core/src/policy.rs` (`Picture::watermark`), `schema/policy.schema.json`
- `crates/veilq-core/src/engine.rs` (`without_watermark`, `watermarks`, test)
- `crates/veilq/src/main.rs` (`--no-watermark`), `crates/veilq/src/ui_settings.rs`, `ui/src/components/FolderRun.vue`

## What this ADR does not say

- That the mark is a signature or a proof of redaction. It is a label.
- That text formats or Office documents get a footer. They stay as they
  were; a ticket body has its own marker (ADR-0024).
