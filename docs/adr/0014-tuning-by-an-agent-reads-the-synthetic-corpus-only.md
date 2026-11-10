# ADR-0014: Tuning by an agent reads the synthetic corpus only

- **Status:** Accepted (card M1, point 6, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [ADR-0013](0013-a-synthetic-corpus-with-truth-and-golden-outputs-gates-recall.md)

## Context

Detection improves fastest when a coding agent runs the evaluation, reads the
misses with their values and fixes the rules. Everything the agent reads goes
to its model provider. Pointed at a real document, that loop sends personal
data off the machine (R8). Pointed at the gate, it makes the numbers green
instead of veilq better.

## Decision

The skill `veilq-tune` (`.claude/skills/veilq-tune/`) works on `eval/` only:
it runs `veilq-eval check` for one source (`--only de/csv`), names the class
of each miss, fixes it in a pack or the engine, adds a test, reruns. It never
reads input from outside `eval/`, never passes the held-out seeds, and never
lowers `eval/gate.toml`, edits a truth file or weakens a look-alike. A real
miss becomes a synthetic template in the generator first.

## Alternatives considered

- **Tuning on real misses directly:** the fastest path, and the one that leaks.
- **No agent in the loop:** slower; the rules are many small regexes.

## Consequences

- The held-out seeds are written in the CI file; the rule is a convention,
  not a secret. A tuning run that uses them is a review finding.

## Enforced by

- `.claude/skills/veilq-tune/SKILL.md`; review of changes to `eval/gate.toml`
  and `eval/**/*.truth.json`.
