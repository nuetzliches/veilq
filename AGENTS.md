# Guidance for AI Assistants

## Core expectations

1. **Target stack:** Rust (latest stable, edition 2024), one workspace under
   `crates/`. `veilq-core` is the engine, `veilq` the CLI and MCP server.
2. **Language:** documentation, commits and code comments in English.
3. **Review card before the code, ADR after it.** A change that fixes an
   outside contract (the JSON schema, the policy file, the placeholder format,
   CLI flags and output that scripts read), moves a trust boundary (what
   leaves the machine, what a model download may do) or adds a user-visible
   mode starts with a review card of three to seven one-sentence points. Show
   it to the owner in the conversation (a table: category, point, what it
   beats, what reversing it costs), get the points approved, then put the card
   and the approval into the pull request. The ADR in `docs/adr/` records what
   was built. See [`docs/adr/README.md`](docs/adr/README.md).
4. **Dependencies:** MIT-compatible licences only (`deny.toml`), latest stable
   versions. A model is a dependency too: its licence goes into the review.
5. **Code style:** `cargo clippy` clean, `cargo fmt` formatted.
6. **Windows is a first-class target.** Every feature works on native Windows
   before it is called done; macOS CI stays green.
7. **Fail closed.** A format, part or detector veilq cannot handle is an
   error. Never pass content through unredacted to keep a run green.

## Personal data in this repository

There is none, and there never will be:

- Test data is synthetic: generated names, addresses, numbers. A value that
  belongs to a real person (a colleague's phone number in a screenshot, a
  customer's IBAN) does not go into a fixture, an issue or a commit message,
  not even redacted.
- Bug reports quote the detector, the entity type and a synthetic
  reproduction, never the input that failed.

## Public-repository hygiene

This repository will become public. It must not contain anything that
identifies or describes a downstream user's product, customers, hosts,
service names, internal paths, ticket numbers or credentials. Describe
downstream experience generically ("a downstream legal-software stack"). A
downstream project's own patterns live in its own policy file, in its own
repository.

## Where things are

| Path | What |
|---|---|
| `docs/plan.md` | Architecture, phases, open questions |
| `docs/adr/` | Decision log |
| `crates/veilq-core` | Extract, detect, decide, apply, verify |
| `crates/veilq` | CLI and MCP server |
