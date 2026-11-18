# ADR-0055: A verify round learns no names, and a field label is no name

- **Status:** Accepted (card in the conversation, points A and B, approved by the owner 2026-11-17); amends [ADR-0047](0047-a-name-found-once-is-found-wherever-the-file-names-it.md)
- **Date:** 2026-11-17
- **Deciders:** project owner
- **Related:** #112, [ADR-0026](0026-a-picture-is-filled-until-a-round-finds-nothing-new.md), [ADR-0052](0052-a-possessive-and-a-noun-are-no-name.md)

## Context

`veilq redact` with a deny value (ADR-0044) for `KD-48213` wrote
`Kundennummer KD-48213.` as `<PERSON> <ACCOUNT_ID>.`; a scan found the
number alone. The verify round reads the output, `Kundennummer
<ACCOUNT_ID>.`, and NER takes `Kundennummer` for a person beside the
placeholder (0.82). The round filled it, as ADR-0026 rules, and the file's
list of names (ADR-0047) learned it, so the second reading found the word
in the input too, everywhere in the file. Of twelve synthetic lines with
placeholders only this one did it; but the mechanism is general: veilq
learned from text it wrote itself.

## Decision

- **A verify round finds the file's names but learns none.** Only readings
  of the input (a text, a cell, a picture's first reading) put a person on
  the list; a verify round of text and a picture read again after its boxes
  were filled read veilq's own output, its placeholders and stand-ins.
  This replaces the narrower rule that kept a stand-in (`fake`) off the
  list.
- **A field label is no name.** A word that ends in `nummer`, `zeichen` or
  `number` (`Kundennummer`, `Aktenzeichen`, `Kennzeichen`, `Customer
  number`) is a role word: a span of only such words is no person, and it
  is never a word of a name on the list.

## Alternatives considered

- **Drop a name a verify round finds beside a placeholder:** would also
  drop a given name the first reading missed beside a surname it found
  (`Lena <PERSON>`): fail open.
- **Leave it:** more is filled, never less; but one label becomes a name
  for the whole file.

## Consequences

- What a verify round finds is still filled where it found it; a name it
  finds that the input's reading missed is no longer looked for elsewhere
  in the file. The input's reading is what the list learns from.
- No surname ending in `nummer` or `zeichen` is known to us; if one turns
  up, it is not learned as a name (NER still finds it as one).
- The corpus and the held-out seeds are unchanged (scan and redaction).

## Enforced by

- `crates/veilq-core/src/engine.rs` (`decided`: no learning in a `verify`
  reading)
- `crates/veilq-core/src/detect/mod.rs` (`LABEL_ENDINGS`, `role_word`)
