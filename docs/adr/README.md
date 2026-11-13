# Architecture Decision Records

A change that touches an architectural point starts with a **review card** in
its pull request, which the project owner approves point by point; the ADR
follows the code and records the decision as built, with the alternatives it
beat.

## The review card

Three to seven points, each one sentence, each tagged with a category, with the
alternative it beats and what reversing it would cost. A point belongs on the
card when it is expensive to reverse **and**

- **trust:** moves a trust boundary (what leaves the machine, what a download
  may do, what an MCP client may read); the point says what an attacker gains
  if it is wrong;
- **concept:** fixes what a core term *is* (a finding, a segment, a policy);
- **contract:** fixes an outside contract (the JSON schema, the policy file,
  the placeholder format, CLI flags and output that scripts read, exit codes);
- **term:** adds a user-visible level, mode, switch or action.

A last line lists what the agent decided on its own.

## Status values

| Status | Meaning |
|---|---|
| `Proposed` | Written, not yet agreed. |
| `Accepted` | Its card points were approved by the owner. Code builds on it. |
| `Recorded` | Written down after the fact; a fact, not a verdict. |
| `Open` | A recorded question with the options laid out. Nothing builds on either side. |
| `Superseded by ADR-NNNN` | Reversed. The file stays. |

## Numbering and format

Four digits, monotonic, never reused. Copy [`0000-template.md`](0000-template.md),
keep it to about a page.

## Index

| ADR | Title | Status |
|---|---|---|
| [0001](0001-pseudonyms-are-a-keyed-hash-in-the-shape-type-hex8.md) | Pseudonyms are a keyed hash in the shape `[TYPE:hex8]` | Accepted |
| [0002](0002-a-finding-never-carries-the-value.md) | A finding never carries the value it found | Accepted |
| [0003](0003-exit-codes-0-1-2-and-veilq-fails-closed.md) | Exit codes 0, 1, 2, and veilq fails closed | Accepted |
| [0004](0004-the-policy-is-veilq-toml-in-the-current-directory-or-the-git-root.md) | The policy is `veilq.toml`, in the current directory or the git root | Accepted |
| [0005](0005-models-are-fetched-explicitly-and-pinned-in-the-binary.md) | Models are fetched explicitly and pinned in the binary | Accepted |
| [0006](0006-four-actions-mask-by-default.md) | Four actions, `mask` by default | Accepted |
| [0007](0007-the-mcp-server-reads-under-one-root-and-http-needs-a-token.md) | The MCP server reads under one root, and HTTP needs a token | Accepted |
| [0008](0008-the-web-ui-is-local-token-bound-and-reads-under-one-root.md) | The web UI is local, token-bound, and reads under one root | Accepted |
| [0009](0009-entity-types-are-locale-neutral-detectors-name-the-locale.md) | Entity types are locale-neutral; detectors name the locale | Accepted |
| [0010](0010-built-in-patterns-are-locale-packs-in-the-policy-format.md) | Built-in patterns are locale packs in the policy's format | Accepted |
| [0011](0011-policy-patterns-take-context-and-a-validator-a-policy-names-its-locales.md) | Policy patterns take context and a validator; a policy names its locales | Accepted |
| [0012](0012-a-failed-check-lowers-the-confidence-it-does-not-drop-the-finding.md) | A failed check lowers the confidence; it does not drop the finding | Accepted |
| [0013](0013-a-synthetic-corpus-with-truth-and-golden-outputs-gates-recall.md) | A synthetic corpus with truth and golden outputs gates recall | Accepted |
| [0014](0014-tuning-by-an-agent-reads-the-synthetic-corpus-only.md) | Tuning by an agent reads the synthetic corpus only | Accepted |
| [0015](0015-scan-and-redact-output-and-exit-codes.md) | `scan` and `redact`: what they print and how they end | Accepted |
| [0016](0016-the-mcp-server-returns-redacted-content-and-reports-only.md) | The MCP server returns redacted content and reports only | Accepted |
| [0017](0017-mcp-over-http-a-token-from-the-environment-loopback-no-browser.md) | MCP over HTTP: a token from the environment, loopback, no browser | Accepted |
| [0018](0018-the-mapping-file-is-age-encrypted-and-revealed-by-the-cli-only.md) | The mapping file is age-encrypted, and revealed by the CLI only | Accepted |
| [0019](0019-office-files-are-edited-in-place-every-part-read.md) | Office files are edited in place, every part read | Accepted |
| [0020](0020-mail-headers-bodies-and-attachments-are-read-and-written-as-mime.md) | Mail headers, bodies and attachments are read and written as MIME | Accepted |
| [0021](0021-the-ui-token-can-be-kept-in-a-file-across-starts.md) | The UI's token can be kept in a file across starts | Accepted |
| [0022](0022-ner-is-gliner-in-onnx-optional-and-pinned.md) | NER is GLiNER in ONNX, optional and pinned | Accepted |
| [0023](0023-pictures-are-read-with-ocr-and-redacted-with-black-boxes.md) | Pictures are read with OCR and redacted with black boxes | Accepted |
| [0024](0024-a-body-carries-its-verdict-in-its-first-line.md) | A body carries its verdict in its first line | Accepted |
| [0025](0025-an-address-cut-short-is-still-an-address.md) | An address cut short is still an address | Accepted |
| [0026](0026-a-picture-is-filled-until-a-round-finds-nothing-new.md) | A picture is filled until a round finds nothing new | Accepted |
| [0027](0027-a-double-click-opens-the-ui-and-setup-installs-for-the-user.md) | A double click opens the UI, and setup installs for the user | Accepted |
| [0028](0028-a-user-policy-edited-in-the-ui-and-folders-redacted-whole.md) | A user policy, edited in the UI, and folders redacted whole | Accepted |
| [0029](0029-veilq-shows-its-ui-in-a-window-of-its-own.md) | veilq shows its UI in a window of its own | Accepted |
| [0030](0030-a-gif-frame-is-read-where-it-changed.md) | A GIF frame is read where it changed | Accepted |
| [0031](0031-a-click-starts-veilq-without-a-console-window.md) | A click starts veilq without a console window | Accepted |
| [0032](0032-the-ui-shows-at-once-and-a-run-waits-for-the-models.md) | The UI shows at once, and a run waits for the models | Accepted |
| [0033](0033-the-ui-is-one-working-surface-for-people-without-a-command-line.md) | The UI is one working surface for people without a command line | Accepted |
| [0034](0034-scan-explains-what-it-read-and-why-it-decided.md) | `scan --explain` says what veilq read and why it decided | Accepted |
| [0035](0035-a-release-is-signed-in-its-own-environment-once-switched-on.md) | A release is signed in its own environment, once switched on | Accepted |
| [0036](0036-input-and-output-are-one-view.md) | Input and output are one view | Accepted |
| [0037](0037-a-misread-checksum-and-a-compound-label-still-redact.md) | A misread checksum and a compound label still redact | Accepted |
| [0038](0038-faces-in-pictures-are-found-and-filled-black.md) | Faces in pictures are found and filled black | Accepted |
| [0039](0039-veilq-is-offered-in-the-microsoft-store-as-msix.md) | veilq is offered in the Microsoft Store as MSIX | Accepted |
| [0041](0041-a-cell-of-a-table-in-a-picture-is-read-whole.md) | A cell of a table in a picture is read whole | Accepted |
