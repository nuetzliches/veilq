# veilq: plan

veilq finds personal data (PII) in content and removes, masks or replaces it:
text, logs, CSV/JSON, Office documents, PDFs, images and screen recordings
(GIF, later video). It is one binary that runs on a developer machine, in any
CI and as an MCP server, so that an agent can redact before it reads,
attaches or sends something.

## 1. Problem

Personal data leaves a system through side doors that no database masking
covers:

- **Exports for analysis:** production telemetry or log exports carry user
  IDs, record IDs, names and addresses into a local analysis tool, a ticket
  or a chat.
- **Attachments:** screenshots, Office files, PDFs and mails are copied from
  one ticket system into another, or into an agent's context.
- **Screen recordings:** a GIF of a bug shows a real customer's record for a
  few frames.
- **Test fixtures and training data:** real documents slip into a repository
  or a fine-tuning set.
- **Agent tool calls:** an agent reads a file, an API answer or a log and the
  content goes to a model provider.

The existing tools either need a Python runtime and a service (Presidio), are
cloud services (the data leaves to be redacted), or handle text only.

## 2. Requirements

| # | Requirement |
|---|---|
| R1 | One static binary for Windows, Linux (x86_64, aarch64) and macOS; no runtime to install. |
| R2 | German first: names, addresses, IBAN with checksum, tax ID, social security number, ID card number, licence plates, court file numbers, dates of birth, phone numbers. English second. |
| R3 | Formats: text, NDJSON, JSON, CSV, DOCX, XLSX, PPTX, EML, PDF, PNG/JPEG/WebP, GIF; video later. |
| R4 | Fail closed: an unsupported format, an unreadable part or a failed detector is an error, never a pass-through. |
| R5 | Every result is serialisable (one JSON schema for CLI and MCP); findings name type, position, confidence and detector, never the original value. |
| R6 | CI: a scan mode with a gate exit code and SARIF output. |
| R7 | Streaming for line-oriented input (pipes, logs, NDJSON, CSV). |
| R8 | Nothing leaves the machine to be redacted. Models are downloaded once, pinned by checksum. |
| R9 | A project defines its own patterns and allow lists in a policy file it keeps in its own repository. |
| R10 | Measured quality: recall and precision per entity type against a synthetic corpus, recall as the CI gate. |

## 3. Architecture

### 3.1 Pipeline

`extract -> detect -> decide -> apply -> verify`

- **extract** turns a container into addressable segments: text spans with
  their location (line/column, cell, XML node, page, frame and box).
- **detect** runs detectors over the segments and returns candidate spans with
  type, confidence and detector name.
- **decide** applies the policy: thresholds, allow lists, which action for
  which type, overlap resolution (the longest, most confident span wins).
- **apply** writes the output in the input's format.
- **verify** runs detection again on the output; a remaining finding above the
  threshold is an error (R4).

### 3.2 Detectors

| Kind | Where | Notes |
|---|---|---|
| Pattern | `veilq-core`, Rust `regex` | Checksums where the format has one (IBAN, tax ID, credit card); context words to raise or lower confidence. |
| Policy pattern | the project's policy file | A project's own identifiers (file numbers, customer numbers). |
| NER | optional, ONNX via `ort` | A multilingual NER model for persons, organisations, locations. Downloaded on first use, pinned by SHA-256, cached; CI caches the model directory. |
| OCR | optional, ONNX via `ort` | Text detection and recognition for images and frames, returning boxes. |
| Verifier | optional, OpenAI-compatible endpoint | A second opinion on low-confidence spans (false positives such as compound nouns read as names). Off by default; only for self-hosted endpoints (R8). |

The binary stays small without NER and OCR; a pattern-only run needs no model.

### 3.3 Actions

| Action | Output | Reversible |
|---|---|---|
| `mask` | `<PERSON>` | no |
| `pseudonymize` | `[PERSON:ab12cd34]`, a keyed hash of the value: the same value gives the same token within one key | with the mapping file (M2) |
| `fake` | a plausible value of the same type and shape | with the mapping file (M2) |
| `remove` | nothing (the span, the cell, the frame region is blanked) | no |

The pseudonym format is `[TYPE:hex8]` from the first release, so that a later
mapping file, or a system that already uses that format, can resolve it. The
mapping file comes in M2, encrypted (age); it never goes into a report.

### 3.4 Formats

| Format | Approach | Traps |
|---|---|---|
| Text, logs, NDJSON, JSON, CSV | streamed in chunks with an overlap window; CSV and JSON aware of columns and keys | a value split across a chunk boundary |
| DOCX, XLSX, PPTX | edit the OOXML parts in place, keep formatting | a value split across runs; comments, tracked changes, headers/footers, document properties (author), hidden sheets, cached formula values, embedded images |
| EML | headers, bodies, attachments recursively | encoded words, nested messages |
| Images | OCR, boxes, fill or blur | EXIF/XMP metadata including GPS |
| GIF | decode frames, OCR changed frames only, track boxes across frames, re-encode | one frame without the box shows the value: boxes persist until the region changes |
| PDF | M4: rasterise pages and redact the images (text layer gone, stated in the report) | incremental updates and metadata keep old content |
| Video | M5: through an external `ffmpeg` | as GIF, plus audio |

### 3.5 Streaming

Streaming is worth it for line-oriented input: `cat app.log | veilq redact -`
starts writing before the input ends and holds constant memory. Container
formats (Office, PDF, images) are read whole; there, MCP progress
notifications report where a long run is. A streamed run cannot verify the
whole output at the end; it verifies each chunk.

## 4. Interfaces

### 4.1 CLI

- `veilq scan <path|->`: findings only, JSON or SARIF, exit code 1 when a
  finding is above the threshold (a CI gate).
- `veilq redact <in|-> -o <out|-> [--policy] [--action]`: the redacted output
  and a report.
- `veilq policy check [<file>]`: validate a policy file.
- `veilq mcp [--http <addr>]`: the MCP server.

### 4.2 MCP

Tools: `scan`, `redact_text`, `redact_file` (a path under an allowed root, or
base64 content), `list_detectors`. Images come back as image content.
Transports: stdio (per user, CI) and Streamable HTTP with a bearer token (a
shared upstream for several agents).

### 4.3 Schema

One JSON schema (`schema/`) for requests, findings and reports, used by the
CLI's `--json` output and by the MCP tools. A finding names the type, the
location, the confidence and the detector; never the original value. A report
says `review_required` when a detector is off that the policy asks for, or
when a format was handled with a known loss (PDF text layer).

### 4.4 Policy file

`veilq.toml` in the consuming repository: entity types and their actions,
thresholds, allow lists (a firm's own name, court names), the project's own
patterns, which detectors are required.

## 5. Phases

| Phase | Content | Done when |
|---|---|---|
| M0 | Repository, review card for the contracts (schema, policy, placeholder, CLI), CI | card approved, CI green |
| M1 | Text, NDJSON, JSON, CSV streamed; pattern detectors for German; `scan`, `redact`, `policy check`; SARIF; evaluation corpus | a log export redacted end to end, recall gate in CI |
| M2 | MCP (stdio + HTTP); mapping file for reversible pseudonyms | an agent redacts an attachment through MCP |
| M3 | DOCX, XLSX, PPTX, EML; metadata stripping; NER | an Office attachment redacted with formatting intact |
| M4 | Images and GIF with OCR; PDF by rasterising | a screen-recording GIF redacted without a leaking frame |
| M5 | Video through ffmpeg | an MP4 screen recording redacted |

## 6. Evaluation

A synthetic corpus with ground truth (`eval/`): generated German and English
records, documents and screenshots. CI measures recall and precision per
entity type; recall is the gate, since a missed phone number costs more than
an extra box. Real personal data never goes into the repository.

## 7. Distribution

As the other nuetzliches tools: GitHub releases for the five targets with
`SHA256SUMS` and build provenance attestations, `install.sh` / `install.ps1`,
a Homebrew formula in `nuetzliches/homebrew-tap`, winget, and a GitHub Action
for CI. Models are release-independent downloads, pinned in the binary.

## 8. Open questions

- O-1: which NER model (licence, German quality, size).
- O-2: which OCR model family (PaddleOCR in ONNX vs. others), and whether its
  quality on UI screenshots is enough.
- O-3: the key for `pseudonymize`: per run, per project (from the policy's
  environment variable) or per mapping file.
- O-4: whether `veilq` also ships rules for an OpenTelemetry collector's
  redaction processor, so the same patterns guard telemetry at ingest.
