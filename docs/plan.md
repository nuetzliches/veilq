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
| Pattern | `veilq-core/locales/<locale>/patterns.toml`, Rust `regex` | Locale packs in the policy's pattern format (ADR-0010): checksums where the format has one, context words and column or key names to raise confidence (ADR-0011, ADR-0012). `intl` always runs; `de` and `en` by default. |
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
| Images | OCR, character boxes filled black, encoded anew (ADR-0023); faces found and filled black (ADR-0038) | EXIF/XMP metadata including GPS; a face the detector misses |
| GIF | decode frames, OCR changed frames only, boxes persist while their pixels stand still, re-encode (ADR-0023) | one frame without the box shows the value |
| PDF | rasterise pages with PDFium and redact the images; a new PDF of page images (text layer gone, stated in the report; ADR-0023) | incremental updates and metadata keep old content |
| Handwriting | M5: a transcript first (3.6), then redacted as a picture | a word misread is a word not found |
| Audio | M6: a transcript first (3.6), each finding's time span bleeped, encoded anew | speech no word was read from; ID3 and other metadata |
| Video | M7 (later, optional): frames as GIF, sound as audio | as GIF, plus audio |

### 3.5 Streaming

Streaming is worth it for line-oriented input: `cat app.log | veilq redact -`
starts writing before the input ends and holds constant memory. Container
formats (Office, PDF, images) are read whole; there, MCP progress
notifications report where a long run is. A streamed run cannot verify the
whole output at the end; it verifies each chunk.

### 3.6 Transcribed media: handwriting and audio

Handwriting and speech are read by a model that makes mistakes a person can
see and fix, and a word read wrongly is a word no detector finds. For these
media the transcript is a stage of its own, reviewed before anything is
redacted:

```
medium -> recogniser -> transcript --[review 1: fix the text]--> detection -> redaction --[review 2: check the findings]--> output
          (HTR / ASR)   words with a box or a                     (as today)   back onto box
                        time span and a confidence                              or time span
```

- **Every word of a transcript keeps its anchor:** a box in the picture or a
  time span in the audio, with the recogniser's confidence and its origin
  (machine or corrected).
- **A corrected word takes the anchor of the words it replaces.** Where a
  correction cannot be placed, the whole line or segment is redacted (fail
  closed).
- **The mode follows the input.** An audio file opens the audio mode; a
  picture or PDF with handwriting (lines read with low confidence, #52, or a
  classifier for print and hand) opens the handwriting mode. A printed form
  filled in by hand is the common case: the transcript shows every line and
  asks to confirm the handwritten and badly read ones.
- **The UI:** handwriting in three parts (original, transcript to edit,
  redacted output), coupled as in ADR-0036 with the line as the anchor;
  audio with a player above the transcript that follows the words and seeks
  on a click, and a player for the redacted audio beside the output. A run
  has two steps: "Confirm the transcript", then the redaction and its
  findings.
- **Without a person** (CLI, MCP) the machine transcript is used and the
  report says `review_required`; a confirmed transcript can be passed in
  (`--transcript <file>`).
- **The output** is the redacted medium and the redacted transcript (text;
  WebVTT for audio). Audio is bleeped over each finding's span plus a
  margin, so the gap is heard; speech without a recognised word is a review
  reason.

**Both reviews feed the learning loop** (#34, #35), as cases of one
versioned format:

| | Review 1: transcript | Review 2: redaction |
|---|---|---|
| Signal | excerpt (picture or audio clip), machine text, corrected text | a mark or a released box (#34) |
| Trains | the recogniser (HTR / ASR: fine-tune or adapter) | rules (#34), the decider (#35) |
| Synthetic twin | handwriting fonts, synthetic speech; counts only if it reproduces the mistake | as in #34 |
| Rings | PC and organisation; upstream only as a twin | PC, organisation, upstream (#35) |

Handwriting and a voice identify the person, not only what they wrote or
said; a voice can be biometric data. Review-1 cases never leave the
organisation, not even encrypted to us.

## 4. Interfaces

### 4.1 CLI

- `veilq scan <path|->`: findings only, JSON or SARIF, exit code 1 when a
  finding is above the threshold (a CI gate).
- `veilq redact <in|-> -o <out|-> [--policy] [--action]`: the redacted output
  and a report.
- `veilq policy check [<file>]`: validate a policy file.
- `veilq mcp [--http <addr>] [--root]`: the MCP server (ADR-0016, ADR-0017).
- `veilq reveal <in> --mapping <file> --identity <file>`: the originals put
  back from a mapping `veilq redact --mapping` wrote (ADR-0018).
- `veilq ui [--root] [--port]`: a local web UI that sets the input (pasted
  text, an upload, a file under the root) beside the redacted output, with
  the findings marked on both sides (ADR-0008).

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
| M0 | Repository, review card for the contracts (schema, policy, placeholder, CLI), CI | **done 2026-11-09**: card approved, ADR-0001 to ADR-0007 |
| M1 | Text, NDJSON, JSON, CSV streamed; pattern detectors for German; `scan`, `redact`, `policy check`; SARIF; evaluation corpus | **done 2026-11-09**: card approved, ADR-0009 to ADR-0015; recall gate in CI on the corpus and five held-out seeds |
| M2 | MCP (stdio + HTTP); mapping file for reversible pseudonyms | **done 2026-11-09**: card approved, ADR-0016 to ADR-0018; an agent redacts a file through MCP (stdio test), mapping and reveal round-trip |
| M3 | DOCX, XLSX, PPTX, EML; metadata stripping; NER | M3a **done 2026-11-09** (card approved, ADR-0019, ADR-0020): Office files and mails redacted in place, metadata replaced, corpus and gate cover them; M3b **done 2026-11-10** (ADR-0022): NER with GLiNER in ONNX, optional, pinned |
| M4 | Images and GIF with OCR; PDF by rasterising | **done 2026-11-10** (card approved, ADR-0023): a screen-recording GIF redacted without a leaking frame; pictures in the corpus, every value covered with NER and OCR |
| M5 | Handwriting (3.6, #59): a handwriting recogniser, the transcript as a stage, the three-part UI, both reviews as cases; needs #52 and the case format (#35) | a hand-filled synthetic form redacted after its transcript was confirmed, both corrections stored as cases |
| M6 | Audio (3.6, #60): a speech recogniser with word timestamps, bleeping, the player in the UI, both reviews as cases | a synthetic recording redacted, its transcript in WebVTT, no finding audible |
| M7 | Video (later, optional): frames as GIF, sound as M6, through an external `ffmpeg` | an MP4 screen recording redacted |

## 6. Evaluation

A synthetic corpus with ground truth (`eval/`): generated German and English
records, documents and screenshots. CI measures recall and precision per
entity type; recall is the gate, since a missed phone number costs more than
an extra box. Real personal data never goes into the repository.

## 7. Distribution

As the other nuetzliches tools: GitHub releases for the four targets (no macOS on
Intel, #53) with
`SHA256SUMS` and build provenance attestations, `install.sh` / `install.ps1`,
a Homebrew formula in `nuetzliches/homebrew-tap`, winget, the Microsoft Store
(MSIX, ADR-0039), and a GitHub Action for CI. Models are release-independent downloads, pinned in the binary.

## 8. Open questions

- O-1 (decided, ADR-0022: `urchade/gliner_multi_pii-v1` in ONNX fp32): which NER model (licence, German quality, size).
- O-2: which OCR model family (PaddleOCR in ONNX vs. others), and whether its
  quality on UI screenshots is enough.
- O-3 (decided, ADR-0018: per project, from the policy's environment
  variable): the key for `pseudonymize`: per run, per project (from the policy's
  environment variable) or per mapping file.
- O-4: whether `veilq` also ships rules for an OpenTelemetry collector's
  redaction processor, so the same patterns guard telemetry at ingest.
- O-5: which handwriting recogniser (TrOCR, Kraken models, a small vision
  language model): licence of the weights, quality on German forms, ONNX
  export, size.
- O-6: which speech recogniser and runtime (Whisper in ONNX Runtime or
  whisper.cpp), and how audio is decoded and written: `symphonia` is
  MPL-2.0 and needs an exception in `deny.toml`, an external `ffmpeg` is
  planned for video anyway.
- O-7: how a recogniser learns from review 1: fine-tuning or an adapter, on
  the PC or only on the organisation's server.
