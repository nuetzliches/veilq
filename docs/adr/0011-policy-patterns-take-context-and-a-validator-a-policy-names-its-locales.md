# ADR-0011: Policy patterns take context and a validator; a policy names its locales

- **Status:** Accepted (card M1, point 3, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [ADR-0004](0004-the-policy-is-veilq-toml-in-the-current-directory-or-the-git-root.md), [ADR-0010](0010-built-in-patterns-are-locale-packs-in-the-policy-format.md), [ADR-0012](0012-a-failed-check-lowers-the-confidence-it-does-not-drop-the-finding.md)

## Context

Most personal data looks like something harmless: a date is a birth date
after `geb.`, eleven digits are a tax ID after `Steuer-ID`, a number is a
phone number in the column `telefon`. A bare regex either misses these or
floods a document with false positives. Checksums (IBAN, tax ID, cards)
separate real identifiers from numbers of the same shape.

## Decision

A pattern (the policy's and a pack's) has, besides `name`, `entity`, `regex`
and `confidence`:

- `context = { words, boost, window }`: when one of `words` is in the
  `window` characters before the match, or in the name of the column or JSON
  key the value is in, `boost` is added to the confidence. Words are compared
  case-insensitively; a word that starts with a letter or digit must start a
  word in the text.
- `validate`: a named check (`iban`, `luhn`, `tax_id_de`, `ssn_de`,
  `id_card_de`, `date`, `ipv4`, `kfz_de`, `ssn_us`).
- A group named `pii` in the regex: only that group is the finding, the rest
  of the match is context it needs (`Frau (?P<pii>Albers)`).

The policy gets `locales`. Of overlapping candidates the longest, then the
most confident wins, before the threshold: a long candidate below it still
claims its span, so that a short rule does not find a phone number among the
digits of an IBAN with a wrong checksum.

## Alternatives considered

- **Regex only:** dates and numbers either all or none.
- **Look-behind in regexes:** Rust's `regex` has none, on purpose (linear
  time); the `pii` group does the same.
- **Context after the match too:** rare in the corpus, and it doubles the
  places a context word can come from; it can be added without breaking files.

## Consequences

- A policy written for M0 still reads: every new key has a default.
- `veilq policy check` compiles the packs and the policy's patterns, so a
  broken regex is found before a run.

## Enforced by

- `crates/veilq-core/src/policy.rs` (`PatternRule`, `Context`, `check`).
- `crates/veilq-core/src/detect/mod.rs` (`has_context`, `findings`), with tests.
- `schema/policy.schema.json`.
