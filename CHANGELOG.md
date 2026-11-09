# Changelog

Notable changes per release. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) as far as a 0.x release can.

## [Unreleased]

### Added

- The repository: plan, workspace, CI.
- The contracts of M0 (ADR-0001 to ADR-0007): the report and policy schemas
  (`schema/`), keyed pseudonyms `[TYPE:hex8]`, exit codes 0/1/2, the policy
  file `veilq.toml` and where it is found, the four actions with `mask` by
  default, the model requirement, and the root a caller's paths stay under.
- `veilq policy check`: validates the policy a run would use and that the
  detectors it requires can run.
- `veilq ui` (ADR-0008): a local web UI that sets the input beside the
  redacted output, from pasted text, an upload or a file under `--root`, with
  each finding marked on both sides. Loopback only, a token per start, `Host`
  and `Origin` checked. Until the detectors of M1 exist it shows that
  redaction is not built yet instead of an output; `npm run dev` in `ui/`
  runs it against a mock with synthetic data.
- `devstack.toml`: the UI's dev server and `veilq ui` under devstack.
- M1: `veilq scan` (text, `--json`, `--sarif`) and `veilq redact` (`-o`
  written only on success, `--action`, `--report`) for text, logs, NDJSON,
  JSON and CSV, streamed; output keeps the input's layout, quoting and
  escapes, and is verified before it is written.
- Pattern detection from locale packs (`intl`, `de`, `en`; ADR-0009,
  ADR-0010): names after a salutation, role, key or signature, addresses,
  birth dates, phone numbers, e-mail, IP, IBAN, cards, tax ID, social
  security and ID card numbers, licence plates, court file numbers.
- Policy: `locales`, and per pattern `context` and `validate` (ADR-0011); a
  failed checksum lowers the confidence instead of dropping the finding
  (ADR-0012).
- The action `fake`: stand-ins of the same type and shape.
- `crates/veilq-eval` and `eval/`: a synthetic corpus with ground truth,
  golden outputs, and a recall gate in CI (ADR-0013); the skill
  `veilq-tune` for tuning against it (ADR-0014).
- M2: `veilq mcp`, the MCP server over stdio or Streamable HTTP (`--http`),
  with the tools `scan`, `redact_text`, `redact_file` and `list_detectors`;
  redacted content and reports only, nothing written, no way back
  (ADR-0016). Over HTTP a bearer token from `VEILQ_MCP_TOKEN` or printed at
  start, loopback hosts, no browser origins (ADR-0017).
- `veilq redact --mapping` writes what `pseudonymize` and `fake` replaced,
  age-encrypted to the policy's `[mapping] recipients`; `veilq reveal` puts
  the originals back (ADR-0018).
