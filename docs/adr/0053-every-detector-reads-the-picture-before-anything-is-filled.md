# ADR-0053: Every detector reads the picture before anything is filled

- **Status:** Accepted ([card](https://github.com/nuetzliches/veilq/issues/108#issuecomment-6957357646), points 1 to 5, approved by the owner 2026-11-17); amends [ADR-0038](0038-faces-in-pictures-are-found-and-filled-black.md) and [ADR-0048](0048-handwritten-signatures-in-pictures-are-found-and-filled-black.md) (regions filled with the first reading's boxes, not before OCR) and [ADR-0045](0045-text-read-badly-is-reviewed-filled-or-stops-the-run.md) (a line inside a region is no reason)
- **Date:** 2026-11-17
- **Deciders:** project owner
- **Related:** #108, #110, [ADR-0026](0026-a-picture-is-filled-until-a-round-finds-nothing-new.md), [ADR-0030](0030-a-gif-frame-is-read-where-it-changed.md)

## Context

A picture's marked areas, faces and signatures were filled black before OCR
read it. In the corpus' form screenshots the signature detector takes a grey
field label (`Steuer-ID`) for a signature (ADR-0048, Consequences); filled
first, the label was gone when OCR read the line, and the tax ID beside it
had lost the context word its pattern needs. A scan fills nothing, so the
gate, which measured scans, never saw it. `veilq-eval check --redact` (#110)
measures a redaction; before this change it found, on the corpus and the
five held-out seeds:

| Run | TAX_ID recall | LICENSE_PLATE recall |
|---|---|---|
| corpus | 0.909 | 0.900 |
| 7001 | 0.833 | 0.857 |
| 7002 | 0.818 | 0.778 |
| 7003 | 0.833 | 0.750 |
| 7004 | 0.818 | 0.909 |
| 7005 | 0.800 | 1.000 |

## Decision

- **One unfilled picture for every detector.** Marked areas (#34), faces
  and signatures are found and reported on the picture as it came; OCR reads
  that same picture. A text candidate inside one of their boxes is theirs and
  not reported again.
- **Filled once, then verified.** The regions are filled with the first
  reading's boxes, and the picture is read again as ADR-0026 rules, until a
  round finds nothing new. A picture with a region and no text finding is
  read twice, where it was read once.
- **A region holds its own lines.** A line OCR read badly inside a face or
  signature box is no review reason and no `UNREAD_TEXT` box (ADR-0045): the
  region is filled whole.
- **GIF frames read in bands** (ADR-0030) take the part of each region that
  lies in a band; each region is filled whole in the frame afterwards.
- **The redaction is a gate.** CI's `gate (redaction)` step fails a run as
  the scan gate does.
- **The decider does not coordinate yet.** Text, face and signature
  findings are not weighed against each other; a learned decider (#35) only
  lifts text candidates.

## Alternatives considered

- **Keep filling first and lower the TAX_ID context requirement:** fixes one
  type and leaves the cause; any label a region takes costs the value beside
  it.
- **Drop signature boxes over text:** would also leave a signature written
  across a printed name (rejected in ADR-0048 already).
- **Read the regions' pixels again after filling:** the filled region holds
  nothing to read; the value outside it was the loss.

## Consequences

- A label taken for a signature is still filled; the value beside it is now
  found too. The false signature boxes stay until the detector is trained
  on form screenshots.
- One more OCR round on pictures with a region and no text finding (see the
  measurement in the pull request).
- A candidate that only overlaps a region is reported and filled on its own,
  as before.

## Enforced by

- `crates/veilq-core/src/visual.rs`: `redact_picture`, `redact_changed`,
  `settle` (`regions`), `in_band`, `unread`
- `.github/workflows/ci.yml`: `gate (redaction)` without
  `continue-on-error`

## What this ADR does not say

- Nothing about how the decider will coordinate findings of different
  detectors.
- Nothing about a better signature model.
- Nothing about running the detectors at once. They still run one after
  the other; reading the same unfilled picture, they no longer depend on
  each other, so a run that hits its limits can read them in parallel.
