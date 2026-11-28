# ADR-0077: The learned decider marks weak findings in pictures

- **Status:** Accepted (review card in #175, six points, approved by the owner in the conversation, 2026-11-27); this records points 1, 2, 5 and 6, and the measurement of point 6 in the description
- **Date:** 2026-11-28
- **Deciders:** project owner
- **Related:** #175, #170, #35, [ADR-0076](0076-the-learned-heads-data-values-in-a-pictures-truth-and-measures-per-candidate.md), [ADR-0047](0047-a-name-found-once-is-found-wherever-the-file-names-it.md)

## Context

veilq's false findings in pictures mostly have one form: a word of the
user interface (a brand, a menu entry, a label) that the NER model takes for
a name, or that the file's names then repeat (ADR-0047). #161 patched these
classes one by one. The spike of #170 showed that a small tree ensemble over
cheap measures of a finding (ADR-0076), trained on the synthetic corpus
truth, removes more of them and loses fewer true names than a
vision-language model, in microseconds and without a server. #175 approved
a second head of the learned decider (#35) that **marks** such findings
first, and drops them only per class, after measurement and with the
owner's approval.

The head was trained in veilq-train (branch `demote/1`): the candidates of
`veilq-eval candidates --only de/png --ner --ocr` over seeds 1 to 200 (35,386
weak findings), the cut chosen on seeds 9001 to 9005, measured on the
held-out seeds 7001 to 7005.

## Decision

1. **A head marks or lifts.** The head format (version 1) holds exactly one
   of `lift_at` (a head that lifts, as before) and `mark_below` (a head
   that marks), each between 0 and 1; a head with both or neither is an
   error. A head that marks lifts nothing, and one that lifts marks
   nothing. A head reads a candidate's named measures (`text.*`, `box.*`,
   `file.repeats`) beside `confidence`, `margin`, `rule=`, `entity=` and
   `detector=`.
2. **What may be marked:** a person's name from a weak detector (`ner`,
   `names:file`) in a picture. Patterns, the policy's lists, unread areas,
   faces and signatures never are. A value is marked only where every
   reading of it in the picture is a weak finding the head marks (a name
   read on the page and again in its line counts once marked only if both
   readings are).
3. **A marked finding stays filled.** The report sets `likely_false: true`
   on it (absent otherwise; `schema/report.schema.json`) and names how many
   were marked among its review reasons, so `review_required` is set. `scan`
   adds "likely no personal data" to its line, SARIF to its message (still
   an error), and the UI lists it apart.
4. **veilq brings the head** (`crates/veilq-core/assets/demote-v0.json`,
   180 KB, `decider::DEMOTE_HEAD`), the card's "or bundled": no download,
   nothing to pin, it changes with the source. It runs wherever OCR runs.
5. **The policy switches it:** `[decider] demote = true` by default;
   `false`, `[decider] enabled = false` or `--no-decider` mark nothing.
6. **One place measures.** The picture's reader computes a candidate's
   measures once (`visual::likely_false`): the shape of its text, the box
   around its rects in shares of the picture, and how often the value is a
   candidate among the picture's readings (`file.repeats`). The head reads
   them there, and `scan --explain` and `veilq-eval candidates` write the
   same numbers, so the next head is trained on what veilq measures.
7. **Measured before it acts.** `veilq-eval check` lists per profile how
   many findings were marked, and recall and precision were they dropped
   (`marked`, `recall-d`, `precision-d`).
8. **Nothing is dropped.** A class may drop its marked findings only when
   the held-out seeds show no true loss for it and the owner approves that
   class (the card's sixth point); that is a later decision.

## Consequences

- On the held-out seeds, per candidate as trained: of 617 true weak
  findings, 2 are marked (0.3 %); of 264 false ones, 253 (96 %).
- Per finding, as `veilq-eval check --only de/png --held-out 7001,…,7005`
  counts it (boxes against the truth's regions): 60 findings marked, 29
  of the 88 false findings among them (`ui` 17 of 25, `headlines` 5 of 5,
  `settings` 5 of 5, `desk` 2 of 48). The other 31 marked boxes touch a
  truth's region (most in `workspace` and `desk`), yet were they dropped,
  every truth would still be covered (recall unchanged in every profile;
  a picture's truth counts as covered at 98 % of its box). The findings
  themselves are unchanged: the gates measure what they measured before.
- Per candidate and per finding differ: a candidate is labelled by its
  value, a finding by whether its box touches a truth, and a value is
  marked only where all its readings are. Before a class drops anything,
  its measure must be the finding's.
- The measures and the candidates of the branch's veilq equal the training
  data: the same 881 weak candidates on the held-out seeds, `file.repeats`
  equal for all 881.
- A picture with a marked finding asks for a review: the reviewer sees the
  likely noise apart instead of hunting for it.
- The head was trained on German pictures of the corpus only (`de/png`);
  on real screenshots it is unmeasured. That is what the marking is for:
  it changes no output.
- A head retrained in veilq-train replaces the asset; its manifest names
  the seeds and the held-out measurement.

## Enforced by

- `crates/veilq-core/src/decider.rs` (`mark_below`, `marks`, `weak`,
  `marked_reason`, `DEMOTE_HEAD`), tests
  `a_head_that_marks_marks_weak_findings_only_and_lifts_nothing`,
  `the_head_veilq_brings_marks_and_reads_what_veilq_measures`,
  `a_head_that_cannot_be_evaluated_is_an_error`
- `crates/veilq-core/src/visual.rs` (`likely_false`), test
  `a_weak_finding_the_head_marks_is_marked_and_still_found`
- `crates/veilq-core/src/engine.rs` (`with_demoter`, `box_measures`, the
  review reason in `report`)
- `crates/veilq-core/src/finding.rs` (`Finding.likely_false`),
  `crates/veilq-core/src/policy.rs` (`DeciderPolicy.demote`),
  `schema/report.schema.json`, `schema/policy.schema.json`
- `crates/veilq-eval/src/check.rs` (`ProfileStat::recall_if_dropped`,
  `precision_if_dropped`)
- `ui/src/components/FindingsPanel.vue`
