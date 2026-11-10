<!-- Synthetic data only: no real names, numbers or screenshots, not even redacted. -->

## What and why

## Review card

<!-- Only when the change fixes an outside contract (schema, policy, placeholder,
CLI output), moves a trust boundary or adds a user-visible mode: three to seven
one-sentence points (category, point, what it beats, what reversing it costs),
approved by the owner. See docs/adr/README.md. Otherwise delete this section. -->

## Checked

- [ ] `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`
- [ ] corpus gate (`veilq-eval check eval`), with `--ner --ocr` if detection changed
- [ ] works on Windows
