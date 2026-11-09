# ADR-0012: A failed check lowers the confidence; it does not drop the finding

- **Status:** Accepted (card M1, point 4, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [ADR-0011](0011-policy-patterns-take-context-and-a-validator-a-policy-names-its-locales.md)

## Context

A checksum tells a real IBAN from a reference number of the same shape. But
people mistype: an IBAN with one wrong digit, next to the word `IBAN`, is
still someone's account, and dropping it leaks it.

## Decision

A match that fails its `validate` check loses 0.3 of its confidence. The
built-in rules are set so that this puts it below the default threshold
(0.7) without context and keeps it at or above with: an IBAN is 0.95 alone,
1.0 after `IBAN`; failed, 0.65 or 0.7. Confidences are rounded to thousandths
so that `1.0 - 0.3` is `0.7`.

## Alternatives considered

- **Drop what fails:** a typo leaks.
- **Ignore the check:** every 22-character code is an IBAN.

## Consequences

- `Konto-Referenz DE89 … 01` (a wrong checksum after `Konto`) is redacted. A
  look-alike that must stay needs no context word near it, or an `allow`
  pattern.

## Enforced by

- `crates/veilq-core/src/detect/mod.rs` (`VALIDATION_PENALTY`, `round`), test
  `a_failed_check_with_context_is_still_found`.
