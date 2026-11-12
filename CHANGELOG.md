# Changelog

Notable changes per release. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) as far as a 0.x release can.

## [Unreleased]

### Added

- veilq can be offered in the Microsoft Store as an MSIX package
  (ADR-0039): the release workflow packs `veilq.exe` (`scripts/msix.ps1`),
  the Store signs and updates it. In a package, veilq keeps its files and
  models in the package's folder, `veilq` on the command line comes from the
  package's alias, and setup, `update` and `uninstall` are left to the Store.
- A picture, GIF or PDF page whose text OCR reads with low confidence
  (handwriting, small or blurred text) is no longer passed silently: where
  a letter or digit scores below 0.7 and nothing was blacked out there, the
  report says `review_required` and where the first such line is (#52).
- Faces in pictures are found and filled black (ADR-0038): images, GIF
  frames, PDF pages and pictures inside Office files and mails. Each face is
  a `FACE` finding, and a picture with faces asks for a review. The face
  detector is the new component `face` (YuNet, MIT, 0.2 MB): `veilq models
  fetch face`. Faces are found, never recognised.

### Changed

- **Pictures need the face detector** unless the policy keeps faces
  (`[picture] faces = "keep"`): without it a picture is refused, as
  without OCR, and an image inside a document falls to `[office] embedded`.

### Fixed

- An IBAN in a picture that OCR reads one character short is redacted
  (ADR-0037). A run of zeros in small print lost a zero, the checksum
  failed, and the IBAN stayed below the threshold. In text read by OCR a
  failed checksum now costs 0.15 instead of 0.3; form checks (dates, IPs,
  plates) cost what they did.
- A context word of five or more characters counts at the end of a compound
  too: `Senderkonto` is context for an IBAN, `Mobiltelefon` for a phone
  number (ADR-0037).
- Three kinds of NER false positives, seen in a screenshot of a news page:
  a shop's domain (`Lesehaus.de`) is no name; what a name owns is cut off
  (`Brandts Kapuze` becomes `Brandts` when `Jonas Brandt` stands in the same
  text or, in a picture, on the same page); a clergy title alone
  (`Der Pater`) is no name, before a name (`Pater Ole`) it stays one.

### Removed

- The macOS Intel build (`veilq-x86_64-apple-darwin.tar.gz`): ONNX Runtime
  publishes none for Intel Macs, so NER, OCR, pictures and PDF could not run
  there (#53). The download page offered it to most Macs, Apple Silicon ones
  too, since a Mac browser reports "Intel" either way; it now offers the
  Apple Silicon build.

## [0.1.9] - 2026-11-12

### Changed

- In the UI, input and output are one view (ADR-0036): they share a zoom
  and the place they show, for text, pictures and PDF pages alike; zoom in
  the input's header or with Ctrl+wheel. A click on a finding in the list
  shows it on both sides, a click on a mark or a frame selects it in the
  list, N and P step through the findings. A finding the panes cannot show
  (inside a picture in a document, on a page past the preview, in a GIF's
  frames) is marked as not in the preview. The full-screen viewer opens on
  the place the panes show, with their zoom.

## [0.1.8] - 2026-11-12

### Added

- `veilq scan --explain` says what veilq read and why it decided
  (ADR-0034): per text read, a picture's OCR lines too, every candidate
  with its detector, confidence, threshold and verdict (found, below
  threshold, overlapped, a role word, a system account, a brand word, …).
  On standard error, beside an unchanged report; it shows the values.
- In the UI, "What veilq read" below the findings runs the input again and
  shows the same, per text read, with a warning that the values are in
  clear.

### Changed

- Releases are ready to be signed (ADR-0035): macOS binaries with a
  Developer ID and notarized, once the release environment holds the keys
  and the switch is on; Windows gets its signing step with the choice of a
  provider. Until then a release's notes say which platform is not signed,
  and the README says how to check a signature.
- The UI is one working surface for people without a command line
  (ADR-0033): the input takes typed or pasted text, a dropped file and a
  screenshot alike, with no tabs to choose first; the findings open in a
  panel beside the output, numbered. Opening a file and redacting a folder
  are in the Open menu; a folder outside the one veilq reads becomes that
  folder. Root, rules in use and version moved to Settings → Advanced.
- The UI speaks German where the system does, English otherwise; Settings
  → General changes it.
- The first start shows a welcome dialog (install, models); Settings is a
  page of its own with General, Detection and Advanced.
- A file refused for a missing model (OCR, PDFium, NER) offers to download
  it where the refusal is shown, and runs again once it is there.

## [0.1.7] - 2026-11-11

### Fixed

- A user's name in a path (`C:\Users\lena.vogt\…`, `/home/lvogt/…`) stayed,
  in a screenshot of a window's title or a settings line too. A home
  directory's name is now a name, unless it is an account the system makes
  (`Public`, `Default`, `root`).
- A product's logo in a screenshot was blacked out as a name: NER took a
  word with a capital inside (`PowerShell`) for a person. Such a word,
  with four letters or more before the capital, is no name for NER now;
  `McDonald` and `DeVries` still are.

## [0.1.6] - 2026-11-11

### Fixed

- A name in a screenshot of a Word document with its formatting marks shown
  was not found: OCR reads the space marks as `-` or `·`, so the name was
  one word with the text before it. A picture's text is read a second time
  with such marks as spaces (only `-` between letters and `·` right between
  two words, so a separator in a letterhead stays).
- A date of birth in a record's header (shown with an icon, no word such as
  `geb.`) stayed: a valid date at least ten years back, beside a found name
  or at the start of the line below it, is now a birth date.
- An address whose street has no street word (`Am Anger 7, 15938 Lindau
  Ost-Heide`) was not found: a street, a number, a comma and a postcode
  make one now, and a town may have up to three words.

### Changed

- The UI shows at once: the models load beside it (ADR-0032), and a run
  started meanwhile waits for them; "Loading models …" says so. The window
  of a double-click veilq opens after half a second instead of two and
  more.

## [0.1.5] - 2026-11-11

### Fixed

- A click on the pinned veilq (or its Start menu entry) opened a terminal
  beside veilq's window, with Windows Terminal as the default terminal. Its
  manifest now tells Windows to make no console for such a start; where
  Windows ignores that, veilq starts itself again without one (ADR-0031).

### Changed

- A GIF frame is read only in bands around the rows that changed against
  the frame before, and whole when those come to more than half of it
  (ADR-0030, #12): a synthetic 131-frame recording at 1920x1080 is
  redacted in 39 s instead of 56 s. `veilq-eval recording` writes it.

## [0.1.4] - 2026-11-11

### Added

- A release has `veilq.exe` on its own too, beside the zip: a double click
  without unpacking, always at `releases/latest/download/veilq.exe`.
- A landing page for people who do not use a command line,
  nuetzliches.github.io/veilq: the download for their system, a
  screenshot, four steps to start.
- The README starts with the same: download, screenshot, four steps.

## [0.1.3] - 2026-11-11

### Changed

- On Windows a double-click veilq shows its UI in a window of its own
  (WebView2, ADR-0029) instead of Edge's app mode: the taskbar and a pinned
  button show veilq and its icon, closing the window ends veilq, and a
  second double click brings it to the front. Without WebView2, the app
  window as before.

## [0.1.2] - 2026-11-11

### Added

- A double click on `veilq` (Windows) opens the web UI in its own window
  and ends a few minutes after the window closed; a second double click
  opens the running one (ADR-0027).
- Settings in the web UI: setup for the user (copy to
  `%LOCALAPPDATA%\Programs\veilq`, Start menu, desktop, `PATH`, "Installed
  apps"), the models with download and progress, and "Quit veilq".
  `veilq uninstall` removes what setup made.
- `veilq.exe` has an icon.
- A user policy (ADR-0028), found after a project's `veilq.toml`:
  `%APPDATA%\veilq\veilq.toml` and the like. Settings edits it as a form:
  action and confidence, languages, per type, never-personal values and
  patterns, own patterns; tried on a sample, saved without a restart.
- Settings chooses the folder the UI reads; the Folder tab redacts every
  file under a folder into a new one, lists what it cannot read (never
  copied) and writes `veilq-report.json`.
- `veilq update` and Settings' "Check for updates": only when asked, the
  newest release from GitHub, checked against its `SHA256SUMS` and put
  where veilq runs; a veilq a double click started restarts into it.
- The web UI copies the redacted output with Ctrl+C when nothing is
  selected, an image as a picture (PNG), text as text; the Copy button too.
  The empty panes say how to drop, paste and copy, and the input pane takes
  a dropped file.

- `veilq redact --unverified` and the web UI's "Generate anyway" (ADR-0026):
  a picture whose verify rounds do not settle is written with the last
  round's findings filled too, and the report asks for a review.

### Changed

- A new mark: lines of text, the middle one blacked out. In the web UI it
  turns over on hover to the nut of the nuts family, and the favicon turns
  with it.
- A picture is read again and filled for up to six rounds instead of three,
  as long as each finds something new (ADR-0026). The error when it does
  not settle names the type and the place (`PERSON at 97x24 at 142,228`).
- NER keeps its candidates per text: a picture's unchanged lines are not
  read by the model again in the next round or frame.

### Fixed

- A GIF is decoded and encoded frame by frame: a screen recording of 717
  frames at 3815x1958 took about 21 GB, every frame decoded at once.

## [0.1.1] - 2026-11-11

### Added

- The web UI takes an image or a file from the clipboard: Ctrl+V uploads it
  as a drop would (a screenshot as `clipboard.png`); text pasted into the
  editor stays plain pasting.
- The web UI shows a picture (an image, a PDF's pages) over the whole window
  to zoom into: fit, natural size, `+`/`-` and Ctrl+wheel, drag to pan,
  fullscreen (`F`), `I`/`O` to switch between input and output, with the
  findings' frames. It opens from the panes' "Zoom" button or a double click.
- An address cut short is still an address (ADR-0025): the `intl` pattern
  `email-cut` finds `name@host` without a top-level domain, as a screen cuts
  it (`mira.albers@exampl…`) or a log names a user on a host. The validator
  `email_host`, which a policy may name too, keeps a package's tag, a
  version, a commit and a digest below the threshold.

### Fixed

- In a picture, a value whose box a neighbour's padded box touched by a few
  pixels was taken for filled already and stayed visible, in the scan and in
  every verify round. A box now covers a value only when it holds nine
  tenths of it.
- German words that mean a person are no names for NER (`Kein Inhalt mit
  Personen.`, `Mitglied`, `Empfänger`, `Teilnehmerinnen`, `Kontaktperson`,
  ...), in their plural, feminine and case forms and at the end of a
  compound; surnames that are such words as well (`Richter`, `Bürger`) stay
  names.

### Changed

- Dependencies: `tokenizers` 0.23, the UI's npm group.

## [0.1.0] - 2026-11-10

### Added

- `veilq mark` (ADR-0024): a ticket's body carries veilq's verdict in its
  first line, `veilq: cleaned|pass <hash>` over the rest's letters and
  digits; a matching body is handed on, a body whose first line is an exempt
  prefix (`[markers] exempt` in the policy) too, any other is redacted and
  marked, its images replaced by a note. The format `markdown` (`.md`)
  writes `mask` as `\<TYPE\>`.

- The repository: plan, workspace, CI.
- The contracts of M0 (ADR-0001 to ADR-0007): the report and policy schemas
  (`schema/`), keyed pseudonyms `[TYPE:hex8]`, exit codes 0/1/2, the policy
  file `veilq.toml` and where it is found, the four actions with `mask` by
  default, the model requirement, and the root a caller's paths stay under.
- `veilq policy check`: validates the policy a run would use and that the
  detectors it requires can run.
- `veilq ui` (ADR-0008): a local web UI that sets the input beside the
  redacted output, from pasted text, an upload or a file under `--root`, with
  each finding marked on both sides. Loopback only, a token per start, `Host`
  and `Origin` checked. Until the detectors of M1 exist it shows that
  redaction is not built yet instead of an output; `npm run dev` in `ui/`
  runs it against a mock with synthetic data.
- `devstack.toml`: the UI's dev server and `veilq ui` under devstack.
- M1: `veilq scan` (text, `--json`, `--sarif`) and `veilq redact` (`-o`
  written only on success, `--action`, `--report`) for text, logs, NDJSON,
  JSON and CSV, streamed; output keeps the input's layout, quoting and
  escapes, and is verified before it is written.
- Pattern detection from locale packs (`intl`, `de`, `en`; ADR-0009,
  ADR-0010): names after a salutation, role, key or signature, addresses,
  birth dates, phone numbers, e-mail, IP, IBAN, cards, tax ID, social
  security and ID card numbers, licence plates, court file numbers.
- Policy: `locales`, and per pattern `context` and `validate` (ADR-0011); a
  failed checksum lowers the confidence instead of dropping the finding
  (ADR-0012).
- The action `fake`: stand-ins of the same type and shape.
- `crates/veilq-eval` and `eval/`: a synthetic corpus with ground truth,
  golden outputs, and a recall gate in CI (ADR-0013); the skill
  `veilq-tune` for tuning against it (ADR-0014).
- M2: `veilq mcp`, the MCP server over stdio or Streamable HTTP (`--http`),
  with the tools `scan`, `redact_text`, `redact_file` and `list_detectors`;
  redacted content and reports only, nothing written, no way back
  (ADR-0016). Over HTTP a bearer token from `VEILQ_MCP_TOKEN` or printed at
  start, loopback hosts, no browser origins (ADR-0017).
- `veilq redact --mapping` writes what `pseudonymize` and `fake` replaced,
  age-encrypted to the policy's `[mapping] recipients`; `veilq reveal` puts
  the originals back (ADR-0018).
- M3a: DOCX, XLSX and PPTX redacted in place, every XML part read (a name
  split across runs found, formatting kept; comments, tracked deletions,
  headers, hidden sheets, cached formula values); author, editor, company
  and custom properties always replaced; images and embedded objects an
  error until M4, or blanked with `[office] embedded = "remove"`
  (ADR-0019).
- EML: headers (encoded words), plain and HTML bodies, attachments
  recursively, written back as MIME (ADR-0020).
- The corpus has DOCX, XLSX and EML documents with their truth.
- M3b: NER (ADR-0022): `urchade/gliner_multi_pii-v1` in ONNX (fp16, from
  Hugging Face; veilq ships no model) finds names that no word marks;
  optional, a report without it asks for a review. Verify adds what it finds
  and replaces it, for up to three rounds.
  `veilq models fetch|install|list`: the model and ONNX Runtime pinned by
  SHA-256, nothing downloaded at build or run time. `veilq-eval check --ner`
  and `eval/gate-ner.toml`.
- `veilq ui --token-file`: the token kept across starts (ADR-0021); under
  devstack an open tab survives a change going live. The UI explains a 401.
- M4 (ADR-0023): images (PNG, JPEG, WebP) read with OCR (PaddleOCR in
  ONNX, `veilq models fetch ocr`), each finding's characters filled black,
  metadata removed by encoding anew; GIF frame by frame with boxes that
  stay while their pixels stand still; PDF rendered with PDFium
  (`veilq models fetch pdf`) into a new PDF of page images; pictures inside
  Office files and mails redacted the same way. A region names its `part`.
  The corpus has a screenshot, a form photo with EXIF, a screen recording
  and a scanned letter; `veilq-eval check --ocr` measures them.
- The UI chooses the action of a run (`mask`, `fake`, `remove`, and
  `pseudonymize` when a key is set) instead of the policy's, as `--action`
  does; a change runs the last source again.
- Releases: archives for Windows, Linux (x86_64, arm64) and macOS with
  `SHA256SUMS` and build provenance; CI measures the corpus with the pinned
  NER, OCR and PDF models too (`eval-models`). README with install, models
  and use; CONTRIBUTING, issue and pull request templates.
- The UI sets an image beside its redacted twin, each finding a frame on
  both sides, and a PDF page by page (its first 20 pages, rendered by
  PDFium in the pixels its findings are in); a GIF is offered as a
  download, with its findings by frame. An image or PDF is shown as soon
  as it is chosen.
