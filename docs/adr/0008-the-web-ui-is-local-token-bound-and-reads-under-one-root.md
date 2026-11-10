# ADR-0008: The web UI is local, token-bound, and reads under one root

- **Status:** Accepted (card UI, points 1-7, approved 2026-11-09); since M1 the UI redacts for real; the token can be kept across starts (ADR-0021)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [plan.md §4.1](../plan.md#41-cli), [ADR-0002](0002-a-finding-never-carries-the-value.md), [ADR-0003](0003-exit-codes-0-1-2-and-veilq-fails-closed.md), [ADR-0007](0007-the-mcp-server-reads-under-one-root-and-http-needs-a-token.md)

## Context

To judge what veilq does with a text, a person wants the input and the output
side by side, with each finding marked on both, from whatever source is at
hand: a pasted snippet, a file from elsewhere, a file in the project. A local
HTTP server is the cheapest way to show that, and it opens a door: any web
page in the same browser can send requests to `127.0.0.1`, and a page whose
DNS name is rebound to 127.0.0.1 can read the answers. Without a guard, that
page could have veilq read the user's files and hand them back.

## Decision

`veilq ui [--root] [--port] [--origin] [--no-open]` serves an embedded web UI
on 127.0.0.1 only. At each start it makes a 256-bit token, prints the URL with
the token in the fragment and opens the browser; the page moves the token into
the tab's session storage and sends it as a bearer token with every API
request. Every request must name the server in `Host` (and in `Origin`, when
sent); `--origin` adds a reverse proxy's address on loopback (devstack), and
nothing else. The UI reads three sources: pasted text, a browser upload, and a
file under `--root`, resolved with `resolve_under` as the MCP server resolves
paths. It fetches no URL and loads no external asset (a CSP of `'self'`). The
two panes are drawn from the report every other interface returns; only the
left one shows the input, from the local source. The HTTP API belongs to the
embedded UI and is not a contract. The UI is Vue 3 with its own CSS, built
with Vite and embedded with `include_dir` behind the default feature `ui`.
Until M1 builds the detectors, `redact_text` fails, and the UI shows that
instead of an output; `npm run dev` runs the UI against an in-browser mock
with synthetic data.

## Alternatives considered

- **A desktop app (Tauri, Electron):** a second distribution and update path
  for a view the browser can show.
- **Loopback without a token:** every web page and local process could use
  it; ADR-0007 rejected the same for MCP over HTTP.
- **Any path, or URLs:** the UI's reach becomes the user's, and a URL source
  makes veilq fetch content from elsewhere (R8).
- **A UI format of its own for findings:** a second shape next to the report,
  and the first place a value could slip into a finding.
- **Nuxt UI (as croniq and nuts-timetrack use):** more weight for two panes
  and a table; the devstack dashboard's plain CSS is enough.
- **Passing the input through until M1:** an output that looks redacted and
  is not (ADR-0003).

## Consequences

- The token changes with every start: a tab opened before a restart gets 401
  and needs the new URL (under devstack: after every change that goes live).
- A release build needs Node to build `ui/` first; without `ui/dist` the
  binary serves a page that says so. `--no-default-features` leaves the UI
  and its dependencies (axum, tokio) out.
- Inputs are UTF-8 text up to 32 MiB until M3; anything else is an error.

## Enforced by

- `crates/veilq/src/ui.rs`: `require_host`, `require_token`, `parse_origin`,
  `redact_source`, with tests (another Host, another Origin, no or a wrong
  token, a path out of the root, binary input, a text and a CSV file
  redacted, a format veilq does not read).
- `crates/veilq-core/src/engine.rs` (`Engine::redact_str`): what the UI shows; a file is read in the format its extension names, and the spans of every finding come from the engine, so cells and JSON values are marked too.
- `ui/src/api/client.ts` (`takeToken`), `ui/src/lib/segments.ts`, with
  tests in `ui/src/lib/segments.test.ts`; CI job `ui`.

## What this ADR does not say

- Nothing about the MCP server's HTTP transport (M2); it keeps ADR-0007.
- The UI does not choose the action: the policy does, as for every run.
