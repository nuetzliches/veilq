# ADR-0040: Purposes are built-in policy packs

- **Status:** Accepted (card in the conversation, points 1 to 7, approved by the owner 2026-11-12)
- **Date:** 2026-11-12
- **Deciders:** project owner
- **Related:** #67, [ADR-0010](0010-built-in-patterns-are-locale-packs-in-the-policy-format.md), [ADR-0028](0028-a-user-policy-edited-in-the-ui-and-folders-redacted-whole.md), #35, #34

## Context

veilq's audience is wider than personal data under the GDPR. A law firm
cares about parties and file numbers, a developer about access keys and
tokens in a log, a press office about faces in a screenshot. What must go
differs, and so does what a mistake costs: in a client's file a miss is
expensive, in a log a class name blacked out breaks the text a developer
needs.

A policy is one file, the first found (ADR-0028): the project's, else the
user's, else the built-in defaults. Policies are not merged, so a second
file per use cannot be the answer. A learned decider (#35) will need,
besides how likely a candidate is real, what each kind of mistake costs;
that is a decision, not something to learn from corrections.

## Decision

- **A purpose is a pack** in `crates/veilq-core/purposes/<name>.toml`,
  compiled into the binary like the locale packs: `about`, and in the
  policy's own format `[entities.TYPE]` (`min_confidence`, `action`),
  `[[patterns]]`, `[allow]`, plus `[costs.TYPE]` (`miss`,
  `false_positive`).
- **A policy names its purposes:** `purposes = ["legal"]`; `general` is
  always one and comes first, the default `["general"]` is veilq's
  behaviour before purposes. `scan` and `redact` take `--purpose` (one or
  more) for a run.
- **Precedence:** the policy's own settings, then the purposes merged.
  Between purposes the stricter rule wins: the lower threshold, the
  stronger action (`remove` before `mask` before `pseudonymize` before
  `fake`), per type the higher cost of a miss and the lower of a false
  positive; allow lists are joined. A purpose's patterns run under their
  own names, `purpose:<purpose>.<rule>`.
- **A purpose only tightens:** its thresholds stay at or below the default
  (0.7), its actions are `mask` or `remove`, it switches no type and no
  face rule off; a pack that would is refused when veilq is tested. It
  loosens only through its allow lists and its costs. Loosening otherwise
  is the policy's own, in its file.
- **Costs are data:** a type no one names costs a miss ten times a false
  positive. No threshold follows from costs yet; the learned decider (#35)
  will use them, and it may move probabilities, never costs.
- **The report names the purposes** of the run (`purposes`, optional,
  `general` first): a coarse label a later case carries, nothing about
  who ran veilq.
- **The UI** asks on the first start what veilq is used for and saves the
  choice to the user's policy at once; Settings → Rules shows each purpose
  with what it changes against `general` (thresholds, patterns, exceptions,
  costs) and saves it with the form.
- **The first packs:** `general` (nothing of its own), `legal` (names and
  addresses from 0.6, court file numbers from 0.5; a miss there costs 20),
  `development` (access keys, GitHub and Slack tokens, private keys, JWTs,
  bearer tokens as `SECRET`; a miss of a secret costs 50, a false positive
  on a name 3). Health, finance and press follow as packs.

## Alternatives considered

- **A profile of the user:** says who someone is, not what must be
  protected; as a label on a case it would be data about the person.
- **A policy file per purpose, layered:** policies are not merged today
  (ADR-0028), and a run would depend on files beside it (as ADR-0010
  rejected for locales).
- **The last named purpose wins:** order would decide protection.
- **Thresholds from costs now:** two sources of one truth, and today's
  behaviour would change with every cost.
- **Packs may loosen (a development pack keeping faces):** protection would
  drop by a click nobody reads.

## Consequences

- A policy and a report have one more field; both schemas grew, and
  every recorded report in `eval/golden` now names `general`.
- A new purpose is a TOML file, its name in `PURPOSES`, its texts in the
  UI; its patterns are checked like any policy's.
- `legal` lowers thresholds: more names and addresses are blacked out,
  some of them by mistake. That is the trade the purpose stands for.
- A purpose cannot make veilq redact less; a team that needs that writes
  it in its own policy, where a reader sees it.

## Enforced by

- `crates/veilq-core/src/purpose.rs` (`PURPOSES`, `purpose`, `merge`,
  `effective`; tests `every_purpose_loads_and_only_tightens`,
  `the_policy_comes_before_its_purposes`)
- `crates/veilq-core/src/policy.rs` (`purposes`, `costs`, `resolved`,
  `cost`), `detect/mod.rs` (`Detector::new` runs the purposes' patterns)
- `crates/veilq-core/src/finding.rs` (`Report::purposes`),
  `schema/policy.schema.json`, `schema/report.schema.json`
- `crates/veilq/src/main.rs` (`--purpose`), `ui_settings.rs` (purposes in
  `/api/policy`), `ui/src/components/PurposesPicker.vue`
