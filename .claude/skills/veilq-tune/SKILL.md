---
name: veilq-tune
description: Improve veilq's detection for one source of the synthetic corpus (a locale and format such as de/csv, en/text) until it is clean - run the evaluation, read the misses and false positives, fix the locale pack or the engine, add a test per fix, rerun. Use it for "tune veilq", "veilq misses X", "improve recall for de/csv", "why is this not redacted", "false positive in the corpus", "add a detector", "make the eval green", and after a new template went into the generator. Works on eval/ only, never on real input. Not for changing the gate, the truth or the generator's look-alikes to make a run pass.
---

# Tuning veilq against the synthetic corpus (ADR-0013, ADR-0014)

The corpus in `eval/` is synthetic: every value is made up, and each document
has a `.truth.json` that names its personal data. That is why its values may
be read, shown and sent to a model here. **Nothing else may**: no file from
outside `eval/`, no pasted customer data, no output of a run on real input.
If someone hands you a real document that veilq gets wrong, write a synthetic
reproduction into the generator first (`crates/veilq-eval/src/generate.rs`),
then tune against that.

## 1. Measure one source

```sh
cargo run -q --release -p veilq-eval -- check eval --only de/csv
```

`--only` takes a path prefix in the corpus (`de`, `de/text`, `en/csv`). The
table gives per type: truth, covered (no byte left in the output), recall,
typed (found as the right type), false positives. Below it each miss and
false positive with file, place and the (synthetic) value. `--json` gives the
same for a script.

Do **not** pass `--held-out`: the held-out seeds are what CI decides on
(`.github/workflows/ci.yml`, job `eval`). Tuning on them makes them worthless.

## 2. Name the class, not the case

For each miss or false positive, say which class it belongs to before you
change anything, e.g.:

- a rule's shape is too narrow (`Best regards` vs `Regards`; `+49 (0)421`),
- a context word is missing (`user=` before a name),
- a short rule finds a piece of a longer value (digits inside an IBAN),
- the look-alike really is ambiguous (a version after `client=`): leave it,
  and say so,
- it needs NER (a name with nothing around it): leave it for M3.

Fix the class: one change that covers every case of it, in the right place:

| Where | What |
|---|---|
| `crates/veilq-core/locales/<locale>/patterns.toml` | patterns, context words, validators of a locale; `intl` for what every locale shares |
| `crates/veilq-core/src/detect/validate.rs` | a new check (checksum, range); list it in `VALIDATORS` |
| `crates/veilq-core/src/detect/mod.rs` | how candidates become findings (overlaps, threshold) |
| `crates/veilq-core/src/engine.rs` | how a format is read and written |

Edit regexes in TOML files with the editor tool, never through a script's
string literal: `\b` in a Python string is a backspace.

## 3. Pin it with a test

Every fix gets a test that fails without it, next to the code it changed
(`detect::tests`, `engine::tests`, `validate::tests`), with synthetic values
only. A rule that should not fire gets a test too (`lookalikes_stay`).

## 4. Rerun everything

```sh
cargo test --workspace
cargo run -q --release -p veilq-eval -- check eval
```

The golden test (`crates/veilq-eval/tests/golden.rs`) fails when an output
changed. Read the diff: an output that redacts more of a truth is expected;
anything else is a regression. Then `VEILQ_UPDATE_GOLDEN=1 cargo test -p
veilq-eval --test golden` and commit the new golden files with the fix.

## What this skill never does

- Lower a value in `eval/gate.toml`, or change a `.truth.json` by hand. The
  gate goes up when veilq gets better; down only with the owner's review.
- Remove or weaken a look-alike in the generator so that a false positive
  disappears.
- Regenerate the committed corpus with another seed to get an easier one.
  New templates are fine (they add cases); regenerate with seed 1 and say so.
- Run on, or quote, anything outside `eval/`.

## Report

What was missed and why (the class), what changed, the recall and precision
before and after for the source, and what is left on purpose (NER, ambiguous
look-alikes).
