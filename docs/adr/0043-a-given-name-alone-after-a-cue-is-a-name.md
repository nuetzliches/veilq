# ADR-0043: A given name alone after a cue is a name

- **Status:** Accepted (card in issue #50, points A1 to A5, approved by the owner 2026-11-13); amends [ADR-0010](0010-built-in-patterns-are-locale-packs-in-the-policy-format.md)'s German pack
- **Date:** 2026-11-13
- **Deciders:** project owner
- **Related:** [ADR-0012](0012-a-failed-check-lowers-the-confidence-it-does-not-drop-the-finding.md), [ADR-0013](0013-a-synthetic-corpus-with-truth-and-golden-outputs-gates-recall.md), [ADR-0042](0042-a-column-of-a-list-names-what-its-cells-hold.md)

## Context

`de.name-role` finds a name after a cue (`Hallo`, `Liebe`, `Bruder`) only
when two capitalised words follow, so that `Liebe Grüße` and `Hallo Welt`
stay out. People write a given name alone: `Hallo Ayşe,`, `mein Bruder
Ali`, and they introduce themselves (`ich heiße Mehmet`, `ich heisse
ayşe`, `Ich bin Lena und …`). NER finds these; without it they stayed in
the output. The corpus had no such text, so the gate did not see it
(#39).

## Decision

1. **After a greeting** (`Hallo`, `Hi`, `Hey`, `Moin`, `Servus`,
   `Liebe/r/s`, `Sehr geehrte/r/s`, `Guten Tag`) one capitalised word is a
   name when a comma, `!`, `:` or the line end follows (`de.name-greeting`,
   0.8).
2. **What follows a greeting without being a name** is a role word
   (`NOT_NAMES`), dropped whoever finds it: `Grüße`, `Kollegen`, `Leute`,
   `Welt`, `Zusammen`, `Alle`, `Freunde`, `Damen`, `Herren` (`Team` was
   one already).
3. **After a kinship word with a possessive** (`mein Bruder`, `meine
   Schwester`, `unser Sohn`, `ihr Mann`, …) one capitalised word is a name
   (`de.name-kin`, 0.8).
4. **A self-introduction** (`ich heiße/heisse/heise`, `mein Name ist`)
   takes the word after it as a name, 0.85 (`de.name-self`), and in lower
   case 0.75 (`de.name-self-lower`).
5. **`ich bin`** takes a capitalised word only before `und`, a comma or the
   line end, 0.75 (`de.name-self-bin`).
6. Points 4 and 5 check the word with a new validator, `given_name_de`,
   that a policy may name too: a word that follows such a cue in a
   sentence (`nicht`, `auch`, a weekday, `heute`, `Urlaub`, …) fails it and
   loses 0.3 (ADR-0012), below the threshold. Months that are given names
   too (`April`, `Juni`, `August`) pass.
7. The corpus gains informal messages (`de/text/chat-*.txt`) with these
   cues, spelling mistakes and greetings without a name, and a case list
   picture (`de/png/list-1.png`, ADR-0042) with surnames alone and cut
   cells; each from a stream of its own, so the documents before stay
   byte for byte.

## Alternatives considered

- **One word after every cue of `name-role`:** `Kontoinhaber Firma`,
  `Mandant Unbekannt`; a role before a record's field is often followed by
  a label, not a name.
- **A stoplist inside each pattern:** the `regex` crate has no
  look-ahead, and the same words are no names for NER either; `NOT_NAMES`
  drops them for every detector.
- **`ich bin` with any word after it:** `Ich bin Lehrer`, `Ich bin
  Montag weg`; the condition on what follows keeps most of them out.

## Consequences

- `Ich bin Lehrer und …` and `Liebe Nachbarin,` are found as names; fail
  closed (AGENTS.md, point 7) takes the false positive.
- A word added to `NOT_NAMES` is no name alone for any detector: a person
  called `Welt` or `Alle` alone stays to a second word.
- A policy can name `given_name_de`; removing it would break such a policy.

## Enforced by

- `crates/veilq-core/locales/de/patterns.toml` (`name-greeting`,
  `name-kin`, `name-self`, `name-self-lower`, `name-self-bin`)
- `crates/veilq-core/src/detect/validate.rs` (`given_name_de`,
  `VALIDATORS`), `crates/veilq-core/src/detect/mod.rs` (`NOT_NAMES`)
- Test `detect::tests::a_given_name_alone_after_a_cue_is_a_name`
- `crates/veilq-eval/src/generate.rs` (`chat_de`),
  `crates/veilq-eval/src/generate/visual.rs` (`list_png`); the gate over
  `eval/`
