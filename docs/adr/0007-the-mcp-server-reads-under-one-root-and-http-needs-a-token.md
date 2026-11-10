# ADR-0007: The MCP server reads under one root, and HTTP needs a token

- **Status:** Accepted (card M0, point 7, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [plan.md §4.2](../plan.md#42-mcp)

## Context

`redact_file` takes a path from an agent. An agent can be steered by the
content it reads (prompt injection); without a limit, veilq becomes the tool
that reads `~/.ssh/id_ed25519` and hands it back. Over HTTP, anything that
reaches the port could do the same.

## Decision

The MCP server resolves every path under `--root` (default: the current
directory), after following every link; `..` and links that lead out are
refused. Over Streamable HTTP it binds to loopback unless told otherwise,
and always requires a bearer token; there is no unauthenticated HTTP mode.

## Alternatives considered

- **Any path the user can read:** the agent's reach becomes the user's.
- **HTTP without a token on loopback:** every local process and every web
  page that reaches loopback could use it.

## Consequences

- An agent that needs a file outside the root passes its content instead.

## Enforced by

- `crates/veilq-core/src/root.rs` (`resolve_under`), with tests (a link out of the root on Unix).
- `crates/veilq/src/mcp.rs` (built in M2): every path through `resolve_under`; the token, host and origin checks as ADR-0017 details them.
