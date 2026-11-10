# ADR-0010: Built-in patterns are locale packs in the policy's format

- **Status:** Accepted (card M1, point 2, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [ADR-0009](0009-entity-types-are-locale-neutral-detectors-name-the-locale.md), [ADR-0011](0011-policy-patterns-take-context-and-a-validator-a-policy-names-its-locales.md)

## Context

veilq starts with German and English and should grow by locale without
changing its core. A project that sharpens detection writes patterns in its
policy; if the built-in ones were written differently (as Rust code), the two
would drift, and a fix in one could not be copied to the other.

## Decision

Each locale has a folder `crates/veilq-core/locales/<locale>/` with
`patterns.toml`: a list of `[[patterns]]` in exactly the format of the
policy's, compiled into the binary. `intl` holds what every locale shares
(e-mail, IBAN, cards, IP, international phone numbers, names after a key or a
mail header) and always runs; the policy's `locales` (default `["de", "en"]`)
choose the others. Rust code is needed only for a check a regex cannot do
(`validate`, ADR-0011) and for data lists such as `locales/de/kfz.txt`.

## Alternatives considered

- **Patterns in Rust:** fast to write, but a second format next to the
  policy's, and every locale is a code change.
- **Packs as files next to the binary:** a run would depend on what is
  installed beside it; a pack could be changed without anyone seeing.

## Consequences

- A new locale is a folder, its name in `LOCALES`, and a corpus in
  `eval/<locale>/`.
- A pack's regex is checked when veilq is tested, not when it is built:
  `detect::tests::every_pack_compiles` guards it.

## Enforced by

- `crates/veilq-core/src/detect/mod.rs` (`PACKS`, `LOCALES`, `pack`).
- `crates/veilq-core/src/policy.rs` (`locales`, checked against `LOCALES`).
