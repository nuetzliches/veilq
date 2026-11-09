# ADR-0009: Entity types are locale-neutral; detectors name the locale

- **Status:** Accepted (card M1, point 1, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [ADR-0010](0010-built-in-patterns-are-locale-packs-in-the-policy-format.md), [plan.md §3.2](../plan.md#32-detectors)

## Context

A German tax ID and a US one are different numbers with the same meaning. A
policy says what happens to a kind of data ("pseudonymize tax IDs"); a report
reader wants to know which rule found it. If the type carried the country
(`DE_TAX_ID`), every policy rule and every report consumer would have to list
each country, and a new locale would change what policies mean.

## Decision

Entity types name the kind of data and are the same in every locale:
`PERSON`, `ADDRESS`, `DATE_OF_BIRTH`, `EMAIL_ADDRESS`, `PHONE_NUMBER`,
`IP_ADDRESS`, `IBAN_CODE`, `CREDIT_CARD`, `TAX_ID`, `SOCIAL_SECURITY_NUMBER`,
`ID_CARD_NUMBER`, `LICENSE_PLATE`, `COURT_FILE_NUMBER`; a policy adds its own.
The finding's `detector` names where it came from: `pattern:<locale>.<rule>`
for a built-in pattern (`pattern:de.tax-id`, `pattern:intl.iban`),
`policy:<name>` for one of the policy.

## Alternatives considered

- **Types per country (`DE_TAX_ID`):** policies grow with every locale, and a
  rule written for one country silently misses the next.
- **The locale as a field of the finding:** a schema change for what the
  detector name already says.

## Consequences

- A policy that treats German and US tax IDs differently cannot do so by type;
  it needs its own pattern with its own type.

## Enforced by

- `crates/veilq-core/src/detect/mod.rs` (`Detector::new` names the detectors).
- `crates/veilq-core/locales/*/patterns.toml`.
