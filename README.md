# veilq

Find and redact personal data in text, documents, images and screen
recordings: on your machine, in CI and as an MCP tool for any agent.

> **Status:** planning (M0). Nothing works yet; see [docs/plan.md](docs/plan.md).

## What it will do

```sh
# CI gate: fail when a fixture contains personal data
veilq scan tests/fixtures --sarif > veilq.sarif

# redact a log export as it streams
cat export.ndjson | veilq redact - --format ndjson > export.redacted.ndjson

# redact a screenshot or a screen recording before attaching it
veilq redact bug.gif -o bug.redacted.gif

# give an agent the same through MCP
veilq mcp
```

- **German first:** names, addresses, IBAN (checked), tax ID, social security
  number, ID card numbers, licence plates, court file numbers, dates of birth,
  phone numbers. English too.
- **Nothing leaves the machine.** Optional models (NER, OCR) are downloaded
  once and pinned by checksum.
- **Fail closed:** what veilq cannot read is an error, never passed through.
- **Serialisable:** one JSON schema for CLI output and MCP tools; findings
  never contain the original value.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
