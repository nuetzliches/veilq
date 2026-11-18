# ADR-0054: A mark acts on its document, is kept as a case, and proposes rules

- **Status:** Accepted ([card](https://github.com/nuetzliches/veilq/issues/34#issuecomment-6954000370), points 1 to 7, approved by the owner 2026-11-17, with 30 days and no excerpt without a key store); amends [ADR-0028](0028-a-user-policy-edited-in-the-ui-and-folders-redacted-whole.md) (the UI writes a rule into the user's policy)
- **Date:** 2026-11-17
- **Deciders:** project owner
- **Related:** #34, #35, [ADR-0011](0011-policy-patterns-take-context-and-a-validator-a-policy-names-its-locales.md), [ADR-0018](0018-the-mapping-file-is-age-encrypted-and-revealed-by-the-cli-only.md), [ADR-0044](0044-a-deny-list-names-the-people-a-text-can-name.md), [ADR-0046](0046-one-threshold-for-one-run-from-a-slider-or-a-flag.md)

## Context

A user who sees veilq leave a value, or black out one that is no personal
data, had no way to say so but editing the policy by hand. Every such
mistake is also what a learned decider (#35) and every later rule should be
tried on, and it is the user's data: what is kept of it, where and for how
long decides whether marking is safe at all. The order of the steps that
check a redaction (#34): first what stays on the machine (mark it yourself,
propose a rule), then the user's agent, the organisation, upstream. This
ADR is the first two.

## Decision

- **A mark acts at once, on this document only.** A value selected in the
  output (text, CSV, JSON) is found as its type wherever the document holds
  it, a box released is left; an area drawn on an image or a PDF page is
  filled and reported (detector `mark`), and what a detector finds inside a
  released box is left in every round of the check. No other file changes.
  A value both marked and released is found (fail closed). An area on a
  GIF's frames is an error, not a mark left out.
- **Every mark is a case** under the user's local data (`%LOCALAPPDATA%\veilq\cases`,
  in the MSIX package its own copy), in the format of
  `schema/case.schema.json`: format, place, kind (missed or extra), type,
  veilq version, policy hash, and what veilq proposed at that place with
  its verdicts and features (`--explain`). The case holds no value.
- **The excerpt is encrypted, for 30 days.** Only the line or cell veilq
  read, or the picture around the area (24 pixels on each side), is kept,
  encrypted with age to an X25519 key in the OS key store (Credential
  Manager, Keychain, Secret Service; `keyring`, service `veilq`, account
  `case-store`), never beside the cases. After 30 days it is deleted; case
  and twin stay. Without a key store nothing of it is written.
- **A twin replaces the values.** A miss marked on text gets a synthetic
  twin: the excerpt with every value veilq replaced and the marked one as
  stand-ins (`fake`). It is kept in the clear only if veilq, without the
  mark, leaves the stand-in as it left the value, and none of the original
  values is in it. A box released gets none: its twin would have to keep
  the released value.
- **A mark proposes rules, the cheapest first**, each tried on the document
  and on every earlier case (its excerpt where the key reads it, else its
  twin) and shown with what it adds and drops:
  - a miss: the value always found as its type (`[[deny]]`); values of its
    shape after the word before it, in its line or its column's name (a
    pattern below every threshold, above them with the word, ADR-0011);
    values of its shape anywhere, only when it holds a digit (a name's
    shape matches nearly every word); its type's threshold lowered to the
    confidence veilq gave it, when a candidate stood there below it;
  - a box released: the value never found (`[allow] values`); values of its
    shape never found, only with a digit (`[allow] patterns`); its type's
    threshold raised above the finding's confidence.
  A shape counts every character when the value holds a digit (`AB-1234`
  is `\p{Lu}{2}\-\d{4}`). A threshold is tried as the policy runs, without
  the run's one threshold, which would stand in its place (ADR-0046).
- **Saved into the user's policy, or copied.** "Add to my rules" writes the
  rule into the user's policy, checked as the settings save it. Where a
  project's `veilq.toml` is in effect, the rule is offered as TOML to copy.
- **Nothing leaves the machine.** "Delete all cases" in the settings
  removes every case.

## Alternatives considered

- **A mark that acts as a rule everywhere at once:** one wrong selection
  would change every file; a rule is a decision of its own.
- **Keeping the whole file, or no excerpt:** the whole file keeps far more
  personal data than the mistake needs; no excerpt leaves nothing to try a
  rule on where no twin can be made.
- **A key file beside the cases, or no encryption:** whoever reads the
  folder reads the excerpts.
- **Training a model from the cases now:** measured in #35, no model
  reaches the rules' recall on templates it has not seen; the cases are
  kept in a format a decider can learn from later.
- **A shape for every value:** for a name it is `\p{Lu}\p{Ll}+`, which
  finds nearly every capitalised word.

## Consequences

- The case store is a folder of personal data's places and, for 30 days,
  encrypted excerpts on the user's machine; "Delete all cases" and the
  retention bound it. The key stays in the OS key store when the cases are
  deleted.
- The retention is fixed at 30 days; the card's "configurable" is not
  built yet.
- A proposed pattern is tried on few texts (the document and the cases): a
  shape rule can find more elsewhere than the counts show. The counts are
  a guide, not a guarantee.
- Marks on Word, Excel and GIF frames, and rules from an area, come later.

## Enforced by

- `crates/veilq-core/src/cases.rs` (case store, encryption, retention,
  twins), `engine.rs` (`with_marks`, `with_picture_marks`, `with_policy`),
  `visual.rs` (marked and released areas)
- `crates/veilq/src/ui_cases.rs` (marks, cases, `Rule`, proposals),
  `ui_settings.rs` (saving a rule into the user's policy)
- `schema/case.schema.json`
- `ui/src/App.vue`, `ui/src/components/CasesCard.vue`

## What this ADR does not say

- Nothing about steps 2 to 4 (the user's agent, the organisation, upstream
  reports) or about a learned decider (#35).
