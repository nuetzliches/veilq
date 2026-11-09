# veilq

Find and redact personal data in text, documents, images and screen
recordings: on your machine, in CI and as an MCP tool for any agent.

> **Status:** M0 done: the contracts stand (`docs/adr/`, `schema/`), only
> `veilq policy check` runs. Detection and redaction come in M1; see
> [docs/plan.md](docs/plan.md).

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

# set the input beside the redacted output in a local web UI
veilq ui
```

- **German first:** names, addresses, IBAN (checked), tax ID, social security
  number, ID card numbers, licence plates, court file numbers, dates of birth,
  phone numbers. English too.
- **Nothing leaves the machine.** Optional models (NER, OCR) are downloaded
  once and pinned by checksum.
- **Fail closed:** what veilq cannot read is an error, never passed through.
- **Serialisable:** one JSON schema for CLI output and MCP tools; findings
  never contain the original value.

## Development

`devstack up` runs the web UI against an in-browser mock (`ui`, :4761) and
`veilq ui` with the UI embedded (`veilq`, :4760; its URL with the token is in
`devstack logs veilq`). Without devstack: `npm ci && npm run dev` in `ui/` for
the mock, `npm run build` there and then `cargo run -- ui` for the real thing.
A build of veilq without `ui/dist` serves a page that says how to get one.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
