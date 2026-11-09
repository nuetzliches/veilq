# ADR-0021: The UI's token can be kept in a file across starts

- **Status:** Accepted (card UI token, points 1 and 2, approved 2026-11-09); amends [ADR-0008](0008-the-web-ui-is-local-token-bound-and-reads-under-one-root.md)
- **Date:** 2026-11-09
- **Deciders:** project owner

## Context

`veilq ui` made a token per start (ADR-0008). Under devstack it starts again
after every build that goes live, so an open tab lost its token with every
change and got 401. What the token guards is the API on loopback, which
lists files under the root and returns their content: against a browser page
the `Host`, `Origin` and content type checks already hold; against another
user or process on the same machine only the token does. It must be secret;
it need not be new at every start.

## Decision

`veilq ui --token-file <path>` reads the token from the file if it holds one
(64 hex characters), else makes a new one and writes it there. Without the
flag a new token per start, as before. The stack file keeps it in
`{repo}/target/devstack/ui-token`: never committed, one per machine and lane,
a new one by deleting the file. On 401 the UI says to open the link veilq
printed (the dashboard's link under devstack).

## Alternatives considered

- **A fixed development token in the stack file or a committed `.env`:** it
  would be public with the repository, and any local user could use the API.
- **The token as a command line value:** visible in the process list.
- **A cookie set at the first visit:** it would need a secret kept across
  starts as well, and one more mechanism.

## Consequences

- Whoever can read the token file can use the UI's API; they can read the
  files under the root directly as well.

## Enforced by

- `crates/veilq/src/ui.rs` (`token`), test `a_token_file_keeps_the_token`.
- `devstack.toml` (`--token-file`), `ui/src/api/client.ts` (the 401 hint).
