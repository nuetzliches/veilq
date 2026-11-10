# ADR-0016: The MCP server returns redacted content and reports only

- **Status:** Accepted (card M2, points 1, 2 and 4, approved 2026-11-09)
- **Date:** 2026-11-09
- **Deciders:** project owner
- **Related:** [ADR-0002](0002-a-finding-never-carries-the-value.md), [ADR-0007](0007-the-mcp-server-reads-under-one-root-and-http-needs-a-token.md), [ADR-0017](0017-mcp-over-http-a-token-from-the-environment-loopback-no-browser.md), [plan.md §4.2](../plan.md#42-mcp)

## Context

An agent should redact before it reads, attaches or sends something. The
agent is also the party veilq protects against: its context goes to a model
provider, and prompt injection can steer what it asks for. Whatever the MCP
server can return, an injected instruction can make it return.

## Decision

`veilq mcp` serves four tools:

- `scan` (a text, a path under `--root`, or base64 content): the report.
- `redact_text` (a text and a format): `output` and the report.
- `redact_file` (a path under `--root`, or base64 content with a name):
  `name`, `output` in the file's format, and the report.
- `list_detectors`: locales, rules (detector, type, locale), the policy's
  patterns, validators, the default action.

Results are structured content; failures are tool errors with the message,
which never quotes the input. The server writes no file, holds no mapping
and has no tool that puts pseudonyms back: an agent gets nothing from it the
redacted output does not already show. It is built on `rmcp`, the official
Rust SDK (Apache-2.0), behind the default feature `mcp`.

## Alternatives considered

- **A tool per format:** more names for one decision the extension makes.
- **`redact_file` writing the output beside the input:** an agent could
  overwrite files under the root; it gets the content and writes it with its
  own tools, where the user's permissions apply.
- **A `reveal` tool:** the shortest path from a prompt injection to the
  original values.
- **The protocol hand-written:** JSON-RPC, sessions and Streamable HTTP are
  much code to keep in step with the specification.

## Consequences

- An agent that needs the originals asks the user, who runs `veilq reveal`.
- Inputs are UTF-8 text formats up to 32 MiB until M3, as in the UI.

## Enforced by

- `crates/veilq/src/mcp.rs`, with tests (output and report only, nothing
  written, a path out of the root, base64 content, an unsupported format).
- `crates/veilq/tests/cli.rs` (`the_mcp_server_answers_over_stdio`: the
  tools listed, none of them `reveal`).
