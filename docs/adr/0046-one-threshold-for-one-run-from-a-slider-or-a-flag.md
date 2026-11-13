# ADR-0046: One threshold for one run, from a slider or a flag

- **Status:** Accepted (card in issue #56, approved by the owner 2026-11-12)
- **Date:** 2026-11-13
- **Deciders:** project owner
- **Related:** [#56](https://github.com/nuetzliches/veilq/issues/56), #52, [ADR-0003](0003-exit-codes-0-1-2-and-veilq-fails-closed.md), [ADR-0028](0028-a-user-policy-edited-in-the-ui-and-folders-redacted-whole.md), [ADR-0034](0034-scan-explains-what-it-read-and-why-it-decided.md), [ADR-0036](0036-input-and-output-are-one-view.md), [ADR-0040](0040-purposes-are-built-in-policy-packs.md)

## Context

A user who sees too much or too little blacked out in one document had one
way to change it: edit the policy's thresholds in Settings → Rules, run
again, and set them back. That changes every later run too, and a threshold
per type is more than someone looking at one letter wants to tune. The
explanation (ADR-0034) already holds, per candidate, its confidence and the
threshold it was held to, so what another threshold would change can be
counted without a run.

## Decision

1. **One threshold for one run.** `Engine::with_min_confidence` (as
   `with_action` for the action) holds every type to one value, in place of
   `defaults.min_confidence` and every type's own threshold, the purposes'
   included. `veilq scan` and `veilq redact` take it as
   `--min-confidence <0..0.95>`; `/api/redact` as `min_confidence` beside
   `action`. The MCP tools do not take it.
2. **The range ends at 0.95.** A finding at 1.0 (a valid checksum, a
   policy's list) never drops out. A value outside 0 to 0.95, or not a
   number, is an error: no output, exit code 2 (ADR-0003), 422 in the UI.
3. **Raising is allowed and said.** Where the run's threshold is above one
   the policy would apply, the report says `review_required`, with one
   reason per raised threshold: "threshold raised from 70 % to 85 % for
   this run" for the default, "threshold for PERSON raised from 60 % to
   65 % for this run" for a type whose own threshold is lower than the
   run's and not the default (a purpose's lowered names, ADR-0040).
   Lowering asks for nothing.
4. **The slider in the output's header** starts at the policy's default
   (`/api/info` gives `min_confidence` and `max_min_confidence`). While it
   is dragged it shows what changes ("+3 / −1 findings"), counted from the
   last run's explanation: a found candidate under the new value drops, one
   below its threshold that reaches it is added. The run is redone on
   release. Above the policy's default the value is marked.
5. **The slider saves nothing.** It holds for the runs of one source;
   another source starts at the policy again. A lasting threshold goes
   through Settings → Rules.

## Alternatives considered

- **A slider per type:** too many controls for a look at one document; the
  policy form has them.
- **A run on every step:** OCR takes seconds per picture; the explanation
  answers the question while dragging.
- **Forbid raising, or raise silently:** forbidding takes away the fix for
  a false positive; raising silently lets a run find less than its policy
  and nobody sees it.
- **A range up to 100 %:** at 1.0 even a valid checksum could drop out.
- **Save the slider's value:** a run's experiment would change every later
  run, past the review of Settings → Rules.
- **UI only:** a user who found the right value could not repeat it in a
  script.

## Consequences

- The UI asks for the explanation with every run, to count while dragging;
  the findings panel still shows it only on its switch. The readings stay
  on loopback (ADR-0034, ADR-0008).
- The count is an estimate: an overlap decided at the old threshold is not
  recounted, and a picture's later verify rounds are not foreseen. The run
  on release decides.
- At the policy's default the slider sends nothing: the policy runs as it
  is, its per-type thresholds included.

## Enforced by

- `crates/veilq-core/src/detect/mod.rs` (`threshold`, `policy_threshold`,
  `raised_thresholds`)
- `crates/veilq-core/src/engine.rs` (`MAX_RUN_CONFIDENCE`,
  `with_min_confidence`, the review reason in `report`)
- `crates/veilq/src/main.rs` (`--min-confidence`), `crates/veilq/src/ui.rs`
  (`RedactRequest::min_confidence`, `Info::min_confidence`)
- `ui/src/lib/threshold.ts` (the count), `ui/src/App.vue` (the slider)
- Tests: `engine::tests::a_threshold_for_one_run_replaces_the_policys_and_a_raised_one_asks_for_a_review`,
  `engine::tests::a_run_threshold_below_the_default_still_raises_a_lower_type`,
  `ui::tests::a_run_can_set_its_threshold_and_a_raised_one_asks_for_a_review`,
  `cli::a_raised_threshold_for_one_run_asks_for_a_review_and_one_out_of_range_fails`,
  `ui/src/lib/threshold.test.ts`
