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
