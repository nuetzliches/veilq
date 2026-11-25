# ADR-0071: Precision patches that change nothing are removed

- **Status:** Accepted (card in the conversation, approved by the owner 2026-11-25: remove group A, keep group B; three of A kept on evidence found while removing, see below)
- **Date:** 2026-11-25
- **Deciders:** project owner
- **Related:** #161, [ADR-0070](0070-the-patches-of-the-detection-are-listed-switchable-and-measured-per-profile.md), [ADR-0001](0001-pseudonyms-are-a-keyed-hash-in-the-shape-type-hex8.md), [ADR-0047](0047-a-name-found-once-is-found-wherever-the-file-names-it.md), [ADR-0055](0055-a-verify-round-learns-no-names-and-a-field-label-is-no-name.md), [ADR-0061](0061-a-name-broken-across-lines-kin-words-and-saints.md), [ADR-0062](0062-points-from-the-review-before-0-7-0.md), [ADR-0067](0067-a-line-read-badly-is-read-again-in-black-and-white.md), [ADR-0069](0069-points-from-the-review-before-0-8-0.md)

## Context

ADR-0070 listed 66 hand-written patches and made each switchable. With
the case of each in the corpus (#163), 34 changed nothing when switched
off on seed 1. Read again over the held-out seeds 7001 to 7005, text and
every picture format, 29 still changed nothing; five did
(`sole-trader`, `legal-forms`, `names-cut-edge`, `table-cell-cut`,
`weak-neighbours`).

The 29 fall in two groups. A patch that drops or cuts a candidate
(precision) can, once removed, at worst add a false finding; a patch that
lifts, adds or forces one (recall) can, once removed, leave a value open
in a case the corpus does not show. veilq fails closed (AGENTS.md). The
owner approved removing the 14 precision patches (A) and keeping the 15
recall patches (B), with the rule that a patch whose removal moves a
number of the gates comes back.

## Decision

These patches go, their code, switch, inventory entry and tests with
them:

| Patch | Was | Brought by |
|---|---|---|
| `not-context` | a context word at the start of `Personalisieren`, `Namespace` gave no boost | #133 |
| `ocr-only-rules` | `name-cut`, `address-glued` ran on OCR text only | ADR-0051, ADR-0066 |
| `label-endings` | a word ending in `nummer`, `zeichen` was a role word | ADR-0055 |
| `saint`, `saint-after-salutation` | `Sankt Martin` was no person; after `Frau` it was | ADR-0061, ADR-0069 |
| `brand-word`, `products` | `PowerShell`, `Copilot` were no person | #32, #79 |
| `genitive-cut` | `Brandts Kapuze` was cut to `Brandts` | #63 |
| `ner-domain-dot` | a NER span with a dot before a lowercase letter (`Lesehaus.de`) was no name | #63 |
| `trade-words` | a firm's trade word was not learned | #130 |
| `contrast-only-where-weak` | a black-and-white re-read counted only where the first reading was weak | ADR-0067 |
| `sole-trader` | an exception to `trade-words`: an `e.K.` owner's trade-like surname was learned; with the skip gone, it is learned anyway | ADR-0062 |

Three of group A stay:

- **`ner-name-shape`** (a NER span must be written like a name). Off
  alone it changed nothing; removed with the others it let `Vater der
  Mandantin`, `Sehr geehrte Damen und Herren` and `Kunde <PERSON>`
  through, and the corpus' precision fell from 0.976 to 0.968. An ablation
  switches one patch off at a time; removing several needs the gates again.
- **`person-nouns`** (`Ansprechpartner`, `Kontaktperson` are no name). Dead
  in a scan; in a redaction its verify round reads `Ansprechpartner` alone
  once the name after it is filled, and NER takes it for one (docx
  precision 1.0 to 0.889). The ablation measured scans only;
  `veilq-eval ablate --redact` now measures a redaction.
- **`placeholder-drop`** (a finding inside veilq's own placeholder is
  dropped). Dead in scan and redaction, but its case is a redacted text
  redacted again, which turned `<DATE_OF_BIRTH>` into `<PERSON>`: the
  placeholder format is a contract (ADR-0001), and the corpus never
  redacts twice. Kept by the agent against the card, for the owner to
  overrule.

Group B stays until a learned structure carries its case (#161):
`compound-tail`, `ocr-checksum-penalty`, `names-glued`, `iban-rest-widen`,
`address-block-pair`, `office-person-metadata`, `particles`,
`ner-saluted-kin` and the context boosts of `de.street-field`,
`de.phone`, `de.social-security`, `de.licence-plate`,
`de.dunning-number`, `intl.phone-international`, `intl.name-field`.

## Alternatives considered

- **Remove all 29**: fewer rules, but a recall patch removed on absent
  evidence can leak a value the corpus does not show.
- **Keep all until the structure exists**: no risk, and no progress on
  what #161 asks for; the measurement exists to act on.

## Consequences

- The gates on the corpus and the held-out seeds, patterns, scan and
  redaction, are as before.
- In documents the corpus does not show, a false finding may appear where
  a patch used to drop it: a product or brand name, a saint's feast, a
  domain, a firm's trade word learned as a name, a word in a
  black-and-white re-read that the first reading had read well. It is
  filled, never left open.
- 54 patches remain switchable (ADR-0070): 33 guard something, 16 are
  dead (group B, `ner-saluted-kin`, `placeholder-drop`), 5 are uncovered.
- A removal is measured as a whole, scan and redaction, not patch by
  patch.

## Enforced by

- `eval/patches.toml`, `crates/veilq-core/src/patch.rs` (`PATCHES`)
- `crates/veilq-eval/src/ablate.rs` (`--redact`)
- the gates in `eval/gate.toml`, `eval/gate-ner.toml`

## What this ADR does not say

- That a recall patch may go on the same evidence: it needs the structure.
- Anything about the five that guard something.
