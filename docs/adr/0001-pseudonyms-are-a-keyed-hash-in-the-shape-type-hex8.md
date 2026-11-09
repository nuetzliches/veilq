# ADR-0001: Pseudonyms are a keyed hash in the shape `[TYPE:hex8]`

- **Status:** Accepted (card M0, point 1, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [plan.md §3.3](../plan.md#33-actions)

## Context

`pseudonymize` replaces a value with a token that stays the same for the same
value, so that a reader can still follow "the same person" through a
redacted log. An existing system in the same environment writes tokens of the
shape `[TYPE:hex8]`, from an unkeyed `sha256(salt:text)`. An unkeyed hash of a
small value space (birth dates, postcodes, phone numbers of one area code) is
reversed by hashing all candidates.

## Decision

veilq writes `[TYPE:hex8]`, where hex8 is the first four bytes of
HMAC-SHA256(key, TYPE, 0x00, normalised value) in lowercase hex. The type is
part of the input. Normalisation: Unicode NFC, lower case, whitespace
collapsed; for IBAN, phone and card numbers, separators removed. A key
shorter than 16 bytes is refused.

## Alternatives considered

- **The existing system's derivation**, for tokens equal across both systems:
  it inherits the brute-force weakness; equal tokens across systems are a
  linkage risk of their own.
- **A longer token** (16 hex): unique enough to be an identifier, but long in
  text a person reads. The mapping file resolves collisions.

## Consequences

- Tokens of veilq and the existing system have the same shape but never the
  same value; a parser reads both.
- 32 bits collide: about 1 % at 9,000 distinct values of one type under one
  key. A token is a label, not a key into a table.
- Changing key, normalisation or derivation changes every token.

## Enforced by

- `crates/veilq-core/src/pseudonym.rs` (`PseudonymKey::pseudonym`, `MIN_KEY_LEN`), with tests.
- Where the key comes from is open (plan O-3); the policy names an environment variable (ADR-0004).
