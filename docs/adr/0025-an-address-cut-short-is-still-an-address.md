# ADR-0025: An address cut short is still an address

- **Status:** Accepted (card in the conversation, points 1 to 3, approved by the owner 2026-11-11)
- **Date:** 2026-11-11
- **Deciders:** project owner
- **Related:** [ADR-0011](0011-policy-patterns-take-context-and-a-validator-a-policy-names-its-locales.md) (amended: one more validator), [ADR-0012](0012-a-failed-check-lowers-the-confidence-it-does-not-drop-the-finding.md), [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md)

## Context

A screen cuts a long value: an account menu shows `mira.albers@exampl…`, and
OCR does not always read the dots. The `email` pattern needs a top-level
domain, so such an address was no finding, and the part before `@`, mostly a
person's name, stayed in the picture. The same shape is in logs
(`root@buildhost-3`, a user on a host). Code has it too, without a person
behind it: a package's tag (`lodash@latest`), a pinned action
(`actions/checkout@v4`, `rust-cache@d1527c8f`), an image's digest
(`node@sha256:…`).

## Decision

- **`email-cut`**, a pattern of the `intl` pack, finds `name@host` without a
  top-level domain as `EMAIL_ADDRESS` (detector `pattern:intl.email-cut`,
  confidence 0.9), in every format, not in pictures only. The host starts
  with a letter and has two characters at least. A full address is still
  found by `email` (0.95), which wins on the same span.
- **`email_host`**, a new validator a policy may name too, fails a host that
  is a tag (`latest`, `next`, `beta`, `stable`, …), a version (`v4`,
  `v1.2`), a commit (seven or more hex digits, one of them a digit) or a
  hash's name (`sha256`, `md5`). Per ADR-0012 the match loses 0.3 and stays
  below the default threshold.

## Alternatives considered

- **Pictures only:** a log or a table cut in the same way would keep the
  name; the engine would need a format switch on patterns, a larger change
  to the policy than one validator.
- **Only with an ellipsis (`…`, `...`):** precise, but OCR loses the dots,
  which is how the case was found.
- **Loosening `email`:** a cut address would then not be told apart in the
  report from a full one, and tags would need the validator there too.

## Consequences

- `user@host` in logs and shell prompts is now a finding; so is any
  `word@word` that is none of the excluded shapes, `weights@inputs` in code
  for one. Fail closed (AGENTS.md, point 7) takes the false positive.
- A policy can name `email_host`; removing it would break such a policy.

## Enforced by

- `crates/veilq-core/locales/intl/patterns.toml` (`email-cut`)
- `crates/veilq-core/src/detect/validate.rs` (`email_host`, `VALIDATORS`)
- Tests: `detect::tests::a_cut_address_is_still_one`,
  `detect::validate::tests::checksums`

## What this ADR does not say

That a picture's other cut values (a name, an IBAN) are found: each needs
its own shape. The fix that let a value touched by a neighbour's box stay
visible (`visual::covered`) is a bug fix, not a decision.
