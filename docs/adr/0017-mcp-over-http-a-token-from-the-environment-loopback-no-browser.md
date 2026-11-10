# ADR-0017: MCP over HTTP: a token from the environment, loopback, no browser

- **Status:** Accepted (card M2, point 3, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [ADR-0007](0007-the-mcp-server-reads-under-one-root-and-http-needs-a-token.md), [ADR-0016](0016-the-mcp-server-returns-redacted-content-and-reports-only.md)

## Context

ADR-0007 requires a bearer token over HTTP and loopback by default. Left
open: where the token comes from, what a server bound to another address
accepts, and what stops a web page in the user's browser.

## Decision

`veilq mcp --http <addr>` serves Streamable HTTP at `/mcp`.

- The token comes from `VEILQ_MCP_TOKEN` (at least 32 characters); without
  it, veilq makes one (256 bits) and prints it once to stderr. There is no
  command line flag for it: arguments are visible in the process list.
- Every request needs `Authorization: Bearer <token>`, compared in
  constant time.
- `Host` must be a loopback name (`localhost`, `127.0.0.1`, `::1`), or the
  address the server was bound to when that is not loopback: DNS rebinding
  gets nothing.
- A request with an `Origin` header is refused: MCP clients send none, a web
  page always does.

## Alternatives considered

- **`--token <value>`:** in `ps`, shell history and CI logs.
- **An allow list of browser origins:** no browser client is needed; a list
  is one more thing to get wrong.

## Consequences

- A shared server for several agents is started with the token in its
  environment; each client gets it from its own secret store.

## Enforced by

- `crates/veilq/src/mcp.rs` (`token`, `router`, `require_token`), test
  `http_needs_the_token_and_no_browser_origin` (no token, a wrong one, a
  browser origin, a rebound host).
