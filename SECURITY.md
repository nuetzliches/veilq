# Security

## Reporting a vulnerability

Please report security problems privately through GitHub: **Security ->
Report a vulnerability** in this repository. Do not open a public issue for
them. You get an answer within a week; a fix and its release are coordinated
with you before anything is published.

A detection miss (a value veilq should have found and did not) is a bug, not
a vulnerability, unless it comes from a flaw that hides whole classes of
input (a format part never read, a detector silently skipped). Report misses
with a **synthetic** reproduction, never with the real input.

## What veilq promises, and what it does not

- **Nothing leaves the machine to be redacted.** veilq makes no network call
  with your content. The only planned network access is the one-time download
  of an optional model, pinned by SHA-256, and an optional verifier endpoint
  you configure yourself.
- **Fail closed.** A format, a part of a document or a detector veilq cannot
  handle stops the run with an error.
- **Reports never contain the original value** of a finding.
- **No detector finds everything.** veilq lowers the risk of personal data
  leaking; it does not remove it. A report says `review_required` when the
  run had a known gap. For content that goes to third parties, a person looks
  at the result.
