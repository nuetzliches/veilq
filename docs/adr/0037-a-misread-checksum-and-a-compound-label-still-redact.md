# ADR-0037: A misread checksum and a compound label still redact

- **Status:** Accepted (card in the conversation, points 1 to 5, approved by the owner 2026-11-12); amends [ADR-0012](0012-a-failed-check-lowers-the-confidence-it-does-not-drop-the-finding.md)
- **Date:** 2026-11-12
- **Deciders:** project owner
- **Related:** [ADR-0011](0011-policy-patterns-take-context-and-a-validator-a-policy-names-its-locales.md), [ADR-0012](0012-a-failed-check-lowers-the-confidence-it-does-not-drop-the-finding.md), [ADR-0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md), [#35](https://github.com/nuetzliches/veilq/issues/35)

## Context

A screenshot of a transfer form showed a German IBAN with a run of zeros,
unspaced, in an 11 px drop-down under the label `Senderkonto *`. veilq left
it standing (exit 0). OCR read the run of zeros one short: 21 characters,
the checksum failed, and ADR-0012 put the IBAN at 0.65, below the threshold
of 0.7. The label would have lifted it to 0.7, but a context word had to
start a word, and `Senderkonto` does not start with `konto`.

Two assumptions of ADR-0012 do not hold for a picture: a failed checksum in
text OCR read says as much about the reading as about the value, and a
German label is often a compound whose head is the context word.

## Decision

1. A context word of five or more characters also counts at the end of a
   word: `Senderkonto`, `Empfängerkonto`, `Mobiltelefon`. A shorter one
   (`ip`, `tel`, `name`) still has to start a word: it sits inside too
   many others (`Zip`, `Dateiname`). Inside a word counts neither.
2. In text read by OCR, a failed **checksum** costs 0.15 instead of 0.3. The
   checksums are `iban`, `luhn`, `tax_id_de`, `ssn_de` and `id_card_de`. An
   IBAN read from a picture is then 0.8 alone and is redacted; a card
   number (0.45), a tax ID (0.5) and a social security number (0.55)
   without context stay below the threshold, as in written text.
3. A **form** check (`date`, `ipv4`, `kfz_de`, `ssn_us`, `email_host`) costs
   0.3 in a picture too: a misread character seldom gives a value the form
   it lacked.
4. The decider of #35 may later learn from these confidences; it does not
   take back a redaction these rules make unless a policy says so.
5. The corpus holds the case: `eval/de/png/transfer-1.png`, a synthetic
   transfer form whose IBAN OCR reads one zero short.

## Alternatives considered

- **Every compound as its own context word** (`senderkonto`, `girokonto`,
  ...): the list is never complete.
- **A context word anywhere inside a word:** `Bankkontonummern` would hold
  `konto`, `Zip` would hold `ip`.
- **A failed IBAN is always an IBAN:** ADR-0012 rejected that for written
  text, where every 22-character code would be one; it stays rejected
  there.
- **Halve the cost of every check in a picture:** an impossible date beside
  `geboren` would become a birth date.
- **Wait for the decider of #35:** a miss leaks now; veilq fails closed.

## Consequences

- A code in a picture that looks like an IBAN and fails its checksum is
  redacted, label or not. A look-alike that must stay needs an `allow`
  pattern.
- Context words at a compound's end lift other detectors too
  (`Mobiltelefon`, `Kontaktperson`); the corpus gate showed no new false
  positive.
- The detector's `candidates`, `findings` and `decide` take the source of
  the text (`Source::Text`, `Source::Ocr`).

## Enforced by

- `crates/veilq-core/src/detect/mod.rs` (`has_context`, `COMPOUND_TAIL`,
  `OCR_CHECKSUM_PENALTY`, `Source`), tests `long_context_words_end_compounds`,
  `a_compound_label_lifts_a_mistyped_iban`,
  `a_failed_checksum_read_by_ocr_costs_less`.
- `crates/veilq-core/src/detect/validate.rs` (`is_checksum`).
- `crates/veilq-core/src/visual.rs` (`ocr_findings` for every OCR reading).
- `crates/veilq-eval/src/generate/visual.rs` (`transfer_png`), gated by
  `eval/gate-ner.toml`.

## What this ADR does not say

- Nothing about OCR quality itself; a reading too poor to trust is #52.
- PDF text layers and office files are text, not OCR: ADR-0012 applies there
  unchanged.
