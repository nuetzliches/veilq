# ADR-0024: A body carries its verdict in its first line

- **Status:** Accepted (card in the conversation, points 1 to 5, approved by the owner 2026-11-10)
- **Date:** 2026-11-10
- **Deciders:** project owner
- **Related:** [ADR-0001](0001-pseudonyms-are-a-keyed-hash-in-the-shape-type-hex8.md) (placeholders), [ADR-0003](0003-exit-codes-0-1-2-and-veilq-fails-closed.md), [ADR-0016](0016-the-mcp-server-returns-redacted-content-and-reports-only.md); devstack's upstream MCP servers (its ADR-0069)

## Context

Agents read tickets: a description from a tracker such as Linear, with the
names and numbers of the people the ticket is about. Whatever an agent
reads is in the model's context, so veilq has to see a body **before** the
agent does, in a gateway between the tracker and the agent; once a body is
redacted, the tracker can keep the redacted version, and the next read
should not need veilq again. Agents also write technical notes into
tickets that need no scan. Trackers render Markdown and store it again in
their own form (escapes, list markers, spaces), keep the history of a
description, and serve images behind short-lived signed URLs.

## Decision

- **The marker** is the body's first line: `veilq: cleaned <hash>` (veilq
  replaced findings) or `veilq: pass <hash>` (it found none). The hash is the
  first eight hex digits of SHA-256 over the rest's **letters and digits
  only**: a tracker that rewrites the Markdown keeps it, an edit that adds a
  name or a number does not. A marker without its hash, or with another one,
  counts for nothing; it is dropped and the body is read again. Neither the
  veilq nor the policy version is in the hash, so an update does not make
  every ticket be written anew.
- **`veilq mark <path|->`** decides on one body and prints JSON: `state`
  (`kept`: its marker matches; `exempt`; `cleaned`; `pass`), `text` (what
  the agent may read), `write` (the body with its marker, to write back,
  when it changed) and the `report`. Exit code 1 when it replaced
  something, 2 on an error. veilq makes no network call: reading from and
  writing to the tracker is the gateway's.
- **Exempt:** a body whose **first** line starts with one of the policy's
  `[markers] exempt` prefixes (an agent's note, `> 🤖 Zu
  Dokumentationszwecken …`) is handed on as it is, unmarked. Only the first
  line: a customer's text with a note appended is read.
- **The format `markdown`** (`.md`, `.markdown`, `--format markdown`) is
  text whose `mask` is written `\<TYPE\>`, so that a renderer shows it
  instead of taking it for an HTML tag; the placeholder filter knows the
  escaped form. Code blocks are read like any text.
- **Images in a body** (`![..](..)`, `<linear-image>`) are behind URLs veilq
  does not fetch: each is replaced by `[image removed: not checked by
  veilq]`, reported as a finding of the type `IMAGE`, and the report asks
  for a review. A gateway that fetches, redacts (ADR-0023) and uploads them
  can come later.

## Alternatives considered

- **A marker without a hash:** any later edit, or a line typed by anyone,
  would let unread text through.
- **A hash over the raw text:** a tracker's Markdown round trip would change
  it on every read, and every read would write the ticket again.
- **veilq fetching the tracker itself:** veilq's promise is that it makes no
  network call with content; a tracker's token does not belong in it.
- **An exempt prefix on any line:** a note appended to a customer's text
  would exempt the text.
- **Images passed through:** a screenshot of a customer's record would reach
  the agent.

## Consequences

- A redacted body replaces the original in the tracker; Linear keeps the
  description's history, where the original stays readable for the
  workspace. The source of truth for personal data is elsewhere (the
  support's own system).
- Screenshots in a ticket are gone for the agent until a gateway redacts
  them.
- The exempt prefix can be typed by anyone who writes the body; it is a
  convention of the team, not a proof of origin.
- Comments are out of scope; a gateway can run them through `veilq mark`
  too, but cannot write them back (only a comment's author can edit it).

## Enforced by

- `crates/veilq-core/src/marker.rs` (hash, states, exemption, images)
- `crates/veilq-core/src/engine.rs` (format `markdown`, placeholder filter)
- `crates/veilq-core/src/policy.rs` (`[markers] exempt`), `schema/policy.schema.json`
- `crates/veilq/src/main.rs` (`veilq mark`), `crates/veilq/tests/cli.rs`

## What this ADR does not say

Where the gateway runs (devstack's upstreams are the candidate), which
tool calls it filters, or how an agent is kept from reading a tracker
directly.
