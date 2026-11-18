# ADR-0056: An area takes a face or a signature, and the policy sets how long excerpts stay

- **Status:** Accepted ([card](https://github.com/nuetzliches/veilq/issues/114), points 1, 5 and 6, approved by the owner 2026-11-18); amends [ADR-0054](0054-a-mark-acts-on-its-document-is-kept-as-a-case-and-proposes-rules.md) (the fixed 30 days)
- **Date:** 2026-11-18
- **Deciders:** project owner
- **Related:** #114, #34, #35, [ADR-0038](0038-faces-in-pictures-are-found-and-filled-black.md), [ADR-0048](0048-handwritten-signatures-in-pictures-are-found-and-filled-black.md)

## Context

An area drawn on a picture offered only the types of text: a face or a
signature veilq missed had to be marked as a name, so the report and the
case said `PERSON` where the picture showed a face. A case that names the
wrong type is a wrong label for whatever later learns from cases (#35).

The excerpts and twins of cases were kept for a fixed 30 days
(`cases::EXCERPT_DAYS`, ADR-0054); an organisation that wants none kept,
or longer for its own review, had no say.

## Decision

- **An area offers `FACE` and `SIGNATURE`** besides the types of text; a
  selection in text does not. The server took any type already; the case
  and the finding carry the type chosen.
- **`[cases] excerpt_days`** in the policy sets how many days a case's
  encrypted excerpt and twin are kept: 30 by default, at most 365 (more is
  an error). `0` keeps neither: the case alone is written, as without a key
  store. The default is not written into a policy's TOML, so a policy's
  hash stays as it was.
- **A shorter time ends what was kept longer.** The store deletes an excerpt
  or a twin when its own time is up or when the store's days, counted from
  when the case was made, are, whichever comes first. Lowering the days, or
  setting `0`, deletes the older excerpts at the next look at the store.
- The settings' "Cases" card says how many days the policy keeps them, or
  that it keeps none.

## Alternatives considered

- **A mark's type for an area free of a list:** a typed type is a type no
  rule or report knows; the list stays the types veilq reports.
- **A new time only for new cases:** an organisation that lowers the time
  wants what is kept gone, not kept on for up to a year.
- **The days in the UI's settings only:** the policy is where the rest of
  what veilq keeps is set, and where a project fixes it for everyone.

## Consequences

- A policy with `[cases]` is a contract change (`schema/policy.schema.json`).
- Raising the days does not lengthen what was kept before: each excerpt
  keeps the time it was written with.
- Marks on GIF frames, on Word and Excel, and rules from an area (card
  points 2 to 4) come in their own changes.

## Enforced by

- `crates/veilq-core/src/policy.rs` (`CasesPolicy`, its check)
- `crates/veilq-core/src/cases.rs` (`Store::keeping`, `purge`)
- `crates/veilq/src/ui_cases.rs` (the store as the policy keeps it)
- `ui/src/lib/marks.ts` (`AREA_TYPES`), `ui/src/components/CasesCard.vue`
- `schema/policy.schema.json`

## What this ADR does not say

- Nothing about cases leaving the machine or training from them (#35).
