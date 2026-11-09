# veilq

Find and redact personal data in text, documents, images and screen
recordings: on your machine, in CI and as an MCP tool for any agent.

> **Status:** M2 done: text, logs, NDJSON, JSON and CSV are scanned and
> redacted with German and English patterns, from the CLI, a local web UI and
> an MCP server; pseudonyms can be put back with an encrypted mapping. Names
> are found by the words around them until NER comes (M3). Office, mail,
> images and PDF follow; see [docs/plan.md](docs/plan.md).

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

# put the originals back, with the key the mapping was encrypted to
veilq redact export.csv -o export.redacted.csv --mapping export.map.age
veilq reveal export.redacted.csv --mapping export.map.age --identity key.txt

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
`veilq ui` with the UI embedded (`veilq`, :4760; the dashboard's link opens it
with its token, `devstack status` lists the URL). Without devstack: `npm ci && npm run dev` in `ui/` for
the mock, `npm run build` there and then `cargo run -- ui` for the real thing.
A build of veilq without `ui/dist` serves a page that says how to get one.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
