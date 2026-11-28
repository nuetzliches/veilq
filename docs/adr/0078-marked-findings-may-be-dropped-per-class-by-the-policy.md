# ADR-0078: Marked findings may be dropped per class, by the policy

- **Status:** Accepted (review card for step 5 in #175, six points, approved by the owner in the conversation, 2026-11-28)
- **Date:** 2026-11-28
- **Deciders:** project owner
- **Related:** #175, [ADR-0077](0077-the-learned-decider-marks-weak-findings-in-pictures.md), [ADR-0047](0047-a-name-found-once-is-found-wherever-the-file-names-it.md)

## Context

ADR-0077 lets the learned decider mark weak findings in pictures as likely
no personal data, and changes no output. Its card left dropping to a later
decision, per class and with the owner's approval. On the held-out seeds the
marks hit most false findings of user interfaces, but some marked boxes touch
true data, and the head has never seen a real screenshot. Dropping therefore
needs a stricter cut than marking, a measure per finding, and a way back for
the reviewer.

## Decision

1. **A class is a weak detector in pictures** (`ner`, `names:file`). The head
   carries per class an optional cut `drop_below`, at most `mark_below`. A
   head that lifts carries none. A finding is dropped when its class is
   named by the policy and it scores below the class's cut. Of several
   readings of one value, the least doubt counts, as for marks.
2. **The policy opts in:** `[decider] drop = ["names:file"]`, empty by
   default. A class the bundled head carries no cut for is an error when
   the policy is loaded. `demote = false`, `enabled = false` and
   `--no-decider` drop nothing.
3. **A dropped finding is reported apart:** it is not filled and not under
   `findings`, which keep meaning "filled". It goes to the report's
   `dropped` list (`schema/report.schema.json`), with a review reason
   naming how many. `scan` adds "not filled: likely no personal data" to its
   line, and SARIF reports it as a `note`. The UI lists it under "not
   filled", with a button that marks its box as missed personal data
   (#34), so it is filled from then on. A container (a document with a
   picture, a mail with an attachment) carries a part's dropped findings
   into its own report, and counts the marked and dropped findings once
   over all its parts.
4. **Measured per finding and per class:** `veilq-eval check` lists per
   class how many findings are marked, how many of them are false, and how
   many truths would be left uncovered were that class alone dropped.
   `check --drop` runs with every class the head may drop dropped. CI runs
   it as the redaction gate over the held-out seeds, with the recall floors
   unchanged. While no class may drop, it checks nothing.
5. **A cut enters the head only by approval:** in a pull request per class,
   when the held-out seeds show no truth lost per finding, and the owner
   approves the class after watching the shadow mode on real screenshots of
   their own. That review stays local; only numbers go into the issue.
6. **Fail closed in the verify rounds:** `redact` reads a picture again
   after filling it. A dropped value is judged again in each round. If a
   later round no longer drops it, it is filled and not listed as dropped.

The head veilq brings carries **no** `drop_below` yet, so nothing changes
until a class is approved.

## Consequences

- Behaviour and gates are unchanged by this decision alone. The first class,
  likely `names:file` repeats of interface words, needs its own pull
  request with the per-class measurement and the owner's approval.
- A policy that names a class before it is approved fails to load, rather
  than silently filling everything.
- Scripts that count `findings` keep counting what was filled. A dropped
  finding is visible in `dropped` and in the review reasons.
- A dropped finding in a picture embedded in a document or mail is listed,
  but the UI's fill button works on a picture or a PDF page shown as such,
  not on an embedded one. There the reviewer marks the area as before.

## Enforced by

- `crates/veilq-core/src/decider.rs` (`drop_below`, `Doubt`, `doubt`,
  `drop_classes`, `dropped_reason`), tests
  `a_class_drops_below_its_cut_only_where_the_policy_names_it`,
  `a_drop_cut_that_cannot_be_used_is_an_error`
- `crates/veilq-core/src/policy.rs` (`DeciderPolicy.drop`), test
  `a_class_to_drop_the_head_may_not_drop_is_an_error`
- `crates/veilq-core/src/finding.rs` (`Report.dropped`,
  `into_all_findings`), `crates/veilq-core/src/engine.rs` (`report`), test
  `a_report_lists_dropped_findings_apart_and_counts_doubts_once`
- `crates/veilq-core/src/visual.rs` (`likely_false`, `settle`,
  `set_aside`), tests
  `a_weak_finding_the_head_marks_is_marked_and_still_found`,
  `a_dropped_finding_a_later_round_filled_is_not_listed_as_dropped`
- `crates/veilq-core/src/office.rs`, `crates/veilq-core/src/mail.rs` (a
  part's report taken whole)
- `crates/veilq-eval/src/check.rs` (`ClassStat`, `--drop`),
  `.github/workflows/ci.yml` (`gate (dropped)`)
- `ui/src/components/FindingsPanel.vue`, `ui/src/App.vue` (`fill`)
