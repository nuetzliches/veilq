# Contributing

Thanks for helping. veilq handles personal data, so a few rules are stricter
than elsewhere.

## No personal data, ever

- **Reproduce with synthetic data.** A bug report or a test quotes the
  entity type, the detector and a made-up value of the same shape, never the
  input that failed, not even redacted. Names from the generator's lists,
  mail under `example.org`, phone numbers in the 555 block, IBANs with valid
  check digits that belong to no one.
- No screenshots of real documents, tickets or applications.
- A security problem goes through GitHub's private vulnerability reporting
  ([SECURITY.md](SECURITY.md)), not an issue.

## Before a pull request

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run --release -p veilq-eval -- check eval --held-out 7001,7002,7003,7004,7005
(cd ui && npm ci && npm run typecheck && npm test)
```

With the models installed (`veilq models fetch ner`, `ocr`, `pdf`), also
`cargo run --release -p veilq-eval -- check eval --ner --ocr --gate eval/gate-ner.toml`.

- **Windows is a first-class target:** a feature works on native Windows
  before it is done.
- **Fail closed:** a format, part or detector veilq cannot handle is an
  error, never a pass-through.
- **Dependencies** carry MIT-compatible licences (`deny.toml`); a model is a
  dependency too.
- **Detection changes** come with a test per fix and a run of the corpus;
  the corpus is tuned on, the held-out seeds are not (ADR-0014). The skill
  `.claude/skills/veilq-tune` describes the loop.

## Decisions

A change that fixes an outside contract (the JSON schema, the policy file,
the placeholder format, CLI flags and output that scripts read), moves a
trust boundary or adds a user-visible mode starts with a short review card
in the pull request and ends with an ADR in [`docs/adr/`](docs/adr); see
[`docs/adr/README.md`](docs/adr/README.md).

Documentation, commits and comments are in English.
