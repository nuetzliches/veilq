# ADR-0070: The patches of the detection are listed, switchable, and measured per profile

- **Status:** Accepted (card in the conversation, five points, approved by the owner 2026-11-25)
- **Date:** 2026-11-25
- **Deciders:** project owner
- **Related:** #161, #39, #35, #150, [ADR-0013](0013-a-synthetic-corpus-with-truth-and-golden-outputs-gates-recall.md), [ADR-0014](0014-tuning-by-an-agent-reads-the-synthetic-corpus-only.md), [ADR-0042](0042-a-column-of-a-list-names-what-its-cells-hold.md), [ADR-0059](0059-a-files-values-firms-as-parties-and-lines-cut-by-the-edge.md)

## Context

Two real screenshots, reported on 2026-11-25 (a news start page and the
record view of an application in a downstream legal-software stack), showed
false positives that no single fix explains: the entries of a side bar under
`Gläubiger` were read as a column of names (`name-column`, ADR-0042), a
headline's compound after a name in the genitive was learned as a name, a
noun that means a person was taken for one, and the file's names
(ADR-0047) repeated each mistake across the page. The learned decider was
not the cause: it may only lift a candidate, and the output is the same
without it.

veilq decides one candidate at a time, from nearby words, once, and repairs
the result with hand-written rules: 46 in the code (word lists such as
`NOT_NAMES` and `PERSON_NOUNS`, cuts such as `genitive_head`, pairings of
lines) and 20 context boosts of the locales' patterns. Many ADRs are one
case each. The direction of #161 is to replace them by a structure that is
learned, not written by hand; a patch may go only when a measurement shows
that nothing that works breaks. The corpus could not show that: the gate
measured totals only (a profile of one picture vanishes in 500 values), no
patch could be switched off, and nobody knew which case each one guards.

## Decision

1. **The patches are listed** in `eval/patches.toml`: id, place, what it
   does, the ADRs and issues that brought it, the case it protects (a
   synthetic example), the corpus files that hold it, and the verdict of
   the last ablation (`guards`, `dead`, `unmeasured`). A test holds the
   list, `veilq_core::patch::PATCHES` and the context boosts of the locales
   equal.
2. **Each patch can be switched off**, at its switch point, by
   `veilq_core::patch::on("<id>")` (a context boost by
   `context:<locale>.<name>`). Only a build with the cargo feature
   `ablation` can switch one off; `veilq-eval` turns it on, the release
   builds `-p veilq` alone, and without the feature `on` is a constant
   `true`. veilq has no flag, policy key or environment variable for it.
3. **`veilq-eval ablate <dir>`** reads the corpus with every patch on, then
   once per patch with it off, and prints the values lost and the false
   findings added, per profile.
4. **`check` measures per profile**: a profile is a document's path
   without the number of its copy (`de/pdf/scan-fax`, `de/png/desk`), with
   its files, values, recall, precision and false findings per file. A
   profile with a section `[profiles."<name>"]` in the gate is measured
   apart: left out of the totals, held at its own `recall` and `precision`.
5. **Two profiles join the corpus**, drawn by the generator with seed 1
   (`de/png/desk-1.png`, `de/png/headlines-1.png`): the record view of a
   debt advice application (a side bar whose entries are the context words
   of `name-column`, two clients with birth date and phone, a table of
   creditors, firms and two persons) and headline tiles (a name in the
   genitive at a line's end, nouns that mean persons, bookmarks). Their
   gate is what main reached on the corpus and the held-out seeds 7001 to
   7005: `desk` recall 1.0, precision 0.5; `headlines` recall 0.66,
   precision 0.8.

The first ablation (corpus of seed 1, scan, NER and OCR, per format):
16 patches guard something, 50 change nothing in the corpus, because
their case is missing from it or they are dead today. The two that read a
column's head (`column-head-context`, `context:intl.name-column`) guard two
or three values of `de/png/list` and make the nine false findings of
`de/png/desk`: one rule, both sides, which a structure has to tell apart.

## Alternatives considered

- **A switch in the shipped binary** (an environment variable): one build
  for the measurement and the release, but a way to turn a protection off
  in the product. Lost: the measurement is the eval's, not the user's.
- **The new profiles in the totals**: honest, but the gate fails on the day
  they are added (precision 0.961 < 0.97) and the existing profiles lose
  their guard. Lowering the totals' floor instead would hide a regression
  elsewhere.
- **The inventory as a table in `docs/`**: readable, but no test could hold
  it equal to the code, and `ablate` could not read it.
- **Real screenshots as fixtures**: they are personal data (AGENTS.md). The
  generator draws the layouts with invented values.

## Consequences

- A new patch costs an entry in `patches.toml`, an id and a switch; the
  `veilq-tune` skill says so, and that it needs the owner's approval.
- The ablation reads the corpus once per patch: with NER and OCR that is
  hours, so it runs by hand (or per source with `--only`), not in CI.
- With the feature on, every switch point reads a lock: `veilq-eval` runs a
  little slower. The release is unchanged.
- A profile measured apart can get worse within its floor unnoticed by the
  totals; its floor goes up when a change makes it better.

## Enforced by

- `crates/veilq-core/src/patch.rs` (`PATCHES`, `on`, `context_on`,
  `switch_off`), the switch points listed in `eval/patches.toml`
- `crates/veilq-eval/src/ablate.rs` (the run and the inventory's tests)
- `crates/veilq-eval/src/check.rs` (`profile`, `per_profile`, the profile
  gate), `eval/gate-ner.toml`
- `crates/veilq-eval/src/generate/visual.rs` (`desk_png`, `headlines_png`)
- `.claude/skills/veilq-tune/SKILL.md`

## What this ADR does not say

- That any patch goes, or how the structure that replaces them is built:
  the next steps of #161, each with its own card.
- Whether a firm in a creditor table without a party's label beside it is a
  party (ADR-0059): the `desk` profile treats it as none.
