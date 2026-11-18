# ADR-0058: A rule from an area names the words OCR read inside it

- **Status:** Accepted ([card](https://github.com/nuetzliches/veilq/pull/122), point 4, approved by the owner 2026-11-18); amends [ADR-0054](0054-a-mark-acts-on-its-document-is-kept-as-a-case-and-proposes-rules.md) (an area proposed no rule)
- **Date:** 2026-11-18
- **Deciders:** project owner
- **Related:** #114, #34, [ADR-0057](0057-an-area-is-marked-on-a-gifs-frame-and-stays-while-its-pixels-do.md)

## Context

An area marked on a picture acted on its document but proposed no rule:
it holds no value a rule could name. A number veilq missed in every
screenshot of the same application was marked again in each one.

## Decision

- **An area names the words OCR reads inside it.** The picture the area is
  drawn on (the image, the GIF's frame, the PDF's page) is read with OCR;
  in the line with most letters inside the area, the words at least half of
  whose letters lie inside are the value. A frame drawn by hand cuts a
  word's edge or touches the next one; a word of no letter or digit (`·`,
  `-`) at an end is left out.
- **The rules are those of that value marked on text** (ADR-0054): always
  found as its type, after the word before it in its line, of its shape
  when it holds a digit, the type's threshold. They are tried on the picture
  itself (its findings with and without the rule, by place) and on the
  earlier cases.
- **Nothing read, nothing proposed.** When OCR reads no word inside the
  area, no rule is proposed and the UI says so; the mark acts all the same.
  Without OCR, a proposal for an area is an error.

## Alternatives considered

- **Every letter whose box lies inside:** a hand-drawn frame cut
  `2026-9081-4214` to `26-9081-4214 - V` on a synthetic screenshot.
- **A rule from the area's place** (always fill this region): it fits one
  layout at one size, and a policy names values, not pixels.

## Consequences

- A proposal for an area reads the picture with OCR once more, then once
  without and once with each rule: a few seconds per rule for a screenshot.
- Earlier picture cases are not tried yet; their excerpts are pictures, and
  rules are tried on the texts of earlier cases only.

## Enforced by

- `crates/veilq-core/src/visual.rs` (`read_area`, `inside`)
- `crates/veilq-core/src/engine.rs` (`Engine::read_area`)
- `crates/veilq/src/ui_cases.rs` (`propose_rule`, `Input`, `found`)
- `ui/src/App.vue` (`offerRule`)
