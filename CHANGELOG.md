# Changelog

Notable changes per release. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) as far as a 0.x release can.

## [Unreleased]

## [0.5.0] - 2026-11-18

### Added

- `veilq-eval check --redact` measures what a redaction finds, not a scan
  (#108): a picture's faces and signatures are filled as a redaction fills
  them, and it is read again. It sees what a filled region costs a text
  detector, which a scan cannot: on the corpus and every held-out seed,
  a signature found on a field label hides the tax ID or licence plate
  beside it. CI gates on it as on the scan.
- Every picture veilq writes (an image, each GIF frame, each PDF page)
  carries a small `veilq <version>` mark at its bottom right (ADR-0050),
  drawn after redaction and verification, so it never hides or causes a
  finding. Images inside Office files and mails stay unmarked. Switch it off
  with `[picture] watermark = false` in the policy, `veilq redact
  --no-watermark` or the folder run's checkbox in the UI.
- `veilq-eval candidates <corpus> --out <file>` writes every candidate the
  detectors proposed, kept or not, as one JSON line with its context and a
  label from the corpus truth (text, CSV, JSON, NDJSON), for measuring the
  models of a learned decider (#95).
- The corpus holds meeting minutes in German and English with hard
  negatives: a ticket number written like a phone number, an order number
  that starts like an international one, a build version, a meeting date
  beside a name. veilq redacts the first two today; they count as false
  positives (precision on the corpus 0.972 with NER, gate 0.97).
- `scan --explain` names the hand rules that hold for each candidate,
  whatever its verdict: `role_words`, `account`, `brand_word`,
  `can_be_birth_date`, `beside_name`, `allowed` (#35). What veilq finds does
  not change; a learned decider will weigh them. `check_failed` names a
  candidate whose pattern's check (a checksum, a date) fails on it.
- The engine can run a learned decider (#35): it may lift a candidate below
  its threshold into the findings, never one whose check failed or that the
  allow lists name, and never drops what a rule keeps; the report then asks
  for a review and says how many it lifted. `--no-decider` (`scan`,
  `redact`) and `[decider] enabled = false` in the policy switch it off.
  None ships yet: until one does, nothing changes.
- The case store in the engine (#34): what a user marks as veilq's
  mistake becomes a case under the user's local data (`veilq\cases`), in
  the format of `schema/case.schema.json`, which holds no value. The line,
  cell or picture crop around the mark is kept only encrypted with age, for
  30 days; without a key nothing of it is written.
- **Marks in the UI** (#34): select text in the output to mark it as
  personal data veilq left, with its type; a finding's "No personal data:
  release here" leaves its box out. A mark acts on this document at once
  and changes no other file; a value both marked and released is found.
  Each mark is kept as a case, the line around it encrypted to a key in the
  OS key store (Credential Manager, Keychain, Secret Service), without that
  line where there is none. Settings → Detection shows the cases and
  deletes them all. Text, CSV and JSON for now; pictures follow.
- **Marks on images and PDF pages** (#34): "Mark an area" in the output
  draws a frame around personal data veilq left; it is filled black and
  reported as its type (detector `mark`). A box on a picture is released from the findings panel: what
  OCR, the face or the signature detector finds inside is left, in every
  round of the check. A case keeps the picture around the area, encrypted.
  Not on a GIF's frames yet: an area marked there is an error, not a mark
  left out, as is one on no page of the input or wholly outside its
  picture.
- **A case's synthetic twin** (#34): a miss marked on text gets a twin, the
  line it was read in with every value veilq found and the marked one as
  made-up stand-ins (`fake`), kept only if veilq, without the mark, leaves
  the stand-in as it left the value; twins are what later rules are tried
  on. What veilq did not find stays in it, so it is kept as the excerpt
  is: encrypted, for 30 days, and not at all without a key store. A box
  released gets none: its twin would have to keep the released value.
- **A mark proposes a rule** (#34): a value marked on text as always found
  as its type (a deny value), a value released as never found (an allow
  value). The proposal says what it changes in the document and in the
  earlier cases (their excerpts where the key reads them, else their
  twins); "Add to my rules" saves it into the user's policy, checked as the
  settings save it. Where a project's `veilq.toml` is in effect, the rule
  is offered as TOML to copy instead.
- **More rules from a mark** (#34, ADR-0054): besides the value, a miss
  proposes values of its shape after the word before it (in its line, or
  its column's name), values of its shape anywhere (only when it holds a
  digit) and its type's threshold lowered to where veilq had it; a box
  released proposes values of its shape never found and its type's
  threshold raised above the finding. The proposal bar lists them, the
  cheapest first, each with what it changes here and in the earlier cases.

### Changed

- Every detector reads a picture before anything in it is filled
  (ADR-0053, #108): marked areas, faces and signatures are found on the
  picture as it came, OCR reads the same picture, and all of it is filled
  once with the first reading's boxes, then verified as before. A field
  label taken for a signature no longer costs the tax ID or licence plate
  beside it: in a redaction, both are found on the corpus and every
  held-out seed. A line read badly inside a face or signature is no longer
  a review reason.

### Fixed

- A field label beside a redacted value no longer becomes a name for the
  whole file (ADR-0055, #112): `Kundennummer KD-48213` with the number
  redacted came out as `<PERSON> <ACCOUNT_ID>`, since the verify round's
  model took `Kundennummer` before the placeholder for a name and the
  file's names learned it. A verify round now learns no names, and a word
  ending in `nummer`, `zeichen` or `number` is no name.
- `Ihre Nachricht`, `Ihr Anliegen` and other pairs of a possessive and one
  word in a sentence are no longer taken for a name (ADR-0052). NER found
  them in subjects such as `Wir haben Ihre Nachricht erhalten`, and the
  names of the file then filled every `Ihre` and `Nachricht` on the page. A
  possessive is never a word of a name: `Ihr Jonas Brandt` under a letter
  is filled as `Jonas Brandt`, and `Ihr` stays readable elsewhere; a
  sign-off keeps its name, on its own line or after the greeting's comma
  (`Viele Grüße, Ihre Lena`).
- A file number after `Aktenzeichen`, `Az.`, `Geschäftszeichen`,
  `Geschäftsnummer` or `Unser/Ihr Zeichen` is found whatever its shape, as
  long as it holds a digit (ADR-0051): a dunning court's `26431907720` or
  `26-4319077-2-0`, a firm's `KL-4471`. Before, only numbers with a slash
  (`3 O 128/26`) were. `Az` counts as a word of its own only (`Azure-VM1`
  is no file number). A dunning court's number is also found after `Mahn…`
  or `Vollstreckungsbescheid` without the word.
- Under the purpose `legal`, a name a screen cut short in a tab or a cell
  (`Albers,…`, `Albers, M ...`) is filled in a picture even where it stands
  nowhere else on the page (ADR-0051). Written text and the default
  threshold are not affected.
- A black box in a picture grows with the line it covers (ADR-0051): a
  quarter of the line's height to the sides, an eighth up and down, at least
  2 pixels as before. The edges of large or bold letters no longer show
  beside and under it; boxes on lines of 12 pixels and more are wider than
  before.
- An IBAN in `[allow] values` is allowed however it is grouped or cased:
  `DE89370400440532013000` in the policy now also leaves `DE89 3704 0044
  0532 0130 00` (as a letterhead or OCR writes it), and the other way round.
  Every other IBAN is still found.
- The UI's two panes keep equal widths, and the findings panel keeps its
  column while a run replaces the result: letting go of the threshold slider
  no longer moves the slider away from the pointer, and a wide picture no
  longer takes more than its half.

## [0.4.1] - 2026-11-16

**Before you update:** the signature detector is a new model; fetch it
again with `veilq models fetch signature` (26 MB), pictures are refused
until then.

### Fixed

- Signatures (ADR-0048, #80): the detector is now veilq-signature-v1,
  YOLOS-tiny fine-tuned by nuetzliches on synthetic letters (Apache-2.0). A
  diagonal watermark across a page (`ENTWURF`, `DRAFT`) is no longer filled
  as a signature, and a signature under a watermark is found: on letters
  with a watermark 20 of 20 signatures against 12, with 14 small false
  boxes against 55. In form screenshots it can fill a small grey field
  label as a signature.

- Small text in pictures (ADR-0049): a picture below 1600 pixels is read
  at up to twice its size, so a scaled-down screenshot's lines are found
  whole. An address whose street OCR read in lower case, an IBAN glued to
  its label (`IBANDE89…`) and a town followed by dots read from `a. M.` are
  found; an IBAN read short is filled as long as it would be, and text
  right of it may be filled with it.

## [0.4.0] - 2026-11-16

**Before you update:** pictures need one more component, the signature
detector (`veilq models fetch signature`, 26 MB); until it is installed an
image, a GIF or a PDF is refused, unless the policy keeps signatures
(`[picture] signatures = "keep"`). A name found once is now found wherever
the same file names it again, and a streamed run may ask for a review it did
not ask for before; `[names] everywhere = false` keeps the old behaviour.

### Added

- Handwritten signatures in pictures (ADR-0048): found by a signature
  detector (YOLOS-tiny, Apache-2.0, converted to ONNX by nuetzliches) and
  filled black as `SIGNATURE` before OCR reads the picture, in images, GIF
  frames, PDF pages and pictures inside Office files and mails. Found, never
  compared. A picture with a signature is `review_required`: large diagonal
  watermark text can be taken for one (#80).
- A file's names (ADR-0047): a person found once in a file (with its pages,
  parts and attachments) is found wherever the file names it again, each
  word of the name alone, in any case and with the genitive, at the
  confidence of the first finding (detector `names:file`). In a picture a
  word cut short (`Jon..`) or cut by a window's left edge (`andtmeier,`)
  counts too. An input held whole is read a second time when a name stood
  before the place it was found; a streamed one asks for a review instead.
  `[names] everywhere = false` turns it off.

### Fixed

- A letter's address block is found: the street on a line of its own and
  the postcode and town on the next (`Am Anger 7` / `15938 Lindau`), in text
  and in a picture, also where a window beside the letter breaks the
  picture's rows.
- A street's small words belong to it: `Auf der Höhe 9, 15938 Lindau` is
  found whole, not from `Höhe` on.
- An international number written with `00` (`0049 30 1234567`) is a phone
  number; one with three leading zeros (`000318/2026`, an invoice's) is not.
- Fewer false names in screenshots: an office under a signature
  (`Geschäftsführer`, `Rechtsanwältin`, `Steuerberater`) and a product in a
  window's bar (`Copilot`) are no person.

## [0.3.0] - 2026-11-13

**Before you update:** a policy that names the purpose `legal` now fills a
line OCR reads badly (handwriting, small or blurred text) black as
`UNREAD_TEXT` instead of only asking for a review; set `[picture]
low_confidence = "review"` to keep the review. A single word under a column
that names a person (`Nachname`, `Gegner`) and a street under `Straße` are
now found, in CSV, XLSX, JSON and pictures, and so is a given name alone
after a greeting (`Hallo Lena,`): expect more findings in lists and messages.

### Added

- Deny lists (ADR-0044, #49): `[[deny]]` in the policy names values that
  are always personal data, inline or in plain text files beside the policy
  (one per line). Each is found as a whole word, in any case, with Turkish
  `İ`/`ı` folded and the genitive (`Mehmets`, `Mehmet's`), and each part of a
  full name alone; at confidence 1.0, even where `allow` names it. A file
  veilq cannot read is an error. The UI shows a list by its size only.
- `[picture] low_confidence` (ADR-0045, #52): what happens to text OCR reads
  badly where nothing was filled. `review` (the default) says so in the
  report as before, `box` fills the line black as `UNREAD_TEXT`, `error`
  stops the run. The purpose `legal` sets `box`; a policy's own value comes
  first.
- One threshold for one run (ADR-0046, #56): a slider in the UI's output
  sets how sure veilq must be to replace something, in place of the
  policy's thresholds, from 0 to 95 %; while it is dragged it shows what
  would change (`+3 / −1 findings`), on release the run is redone. `scan`
  and `redact` take the same as `--min-confidence`, `/api/redact` as
  `min_confidence`. A threshold above the policy's makes the report ask
  for a review (`threshold raised from 70 % to 85 % for this run`). It
  saves nothing; a lasting threshold goes through Settings → Rules.
  `--explain` marks a check's reading (a verify round, a picture read
  again) as `read again:` (`"verify": true` in JSON), and the slider's
  preview counts each candidate once.
- Purposes (ADR-0040, #67): what veilq is used for, as packs built into
  veilq. A policy names them (`purposes = ["legal", "development"]`), a run
  takes `--purpose`; `general` always applies and is today's behaviour.
  `legal` lowers the thresholds for names, addresses and court file
  numbers; `development` finds secrets (`SECRET`: AWS access keys, GitHub
  and Slack tokens, private keys, JWTs, bearer tokens). A purpose only
  tightens, the policy's own settings come first, and each purpose states
  what a miss and a false positive cost, for a later learned decider. The
  report names the purposes of the run. The UI asks on the first start and
  shows each purpose's rules under Settings → Rules.

### Fixed

- "Redact folder" in the UI says why it cannot start yet (#68): until a
  source is chosen, its "Choose …" is the primary button and a hint beside
  the start button names what is missing, for "From" and for "To".
- A given name alone after a cue is found without NER (ADR-0043, #50):
  after a greeting when a comma, `!` or the line end follows (`Hallo Ayşe,`),
  after a kinship word (`mein Bruder Ali`) and in a self-introduction
  (`ich heiße Mehmet`, `ich heisse ayşe`, `Ich bin Lena und …`). The corpus
  gains informal messages and a case list picture with surnames alone and
  cut cells (#39).
- A table cell in a picture whose text wraps over several lines is read as
  one text (ADR-0041). Before, its middle line was read in the row of the
  cells beside it, and a word of a company name there (`KMS Bad` /
  `Kissinger` / `Servicegesellschaft`) was taken for a surname and filled
  black on its own.
- A list's columns name what their cells hold (ADR-0042). A surname alone
  under a column such as `Nachname` or `Gegner`, and a street under
  `Straße`, also cut short (`Hachenburg …`), are found, in CSV, XLSX, JSON
  and in pictures, where a cell now reads the head of its table's column.
  A picture's row with street, number, postcode and town in cells of their
  own is an address, and so is a street whose street word stands alone
  (`Trebuser Str. 3d 15517 Fürstenwalde`). Before, all of these stayed.
- A name inside a value that a weaker rule took whole (`Herr Jonas
  Brandt` as a field) is found again: a candidate below its threshold no
  longer hides a finding of its own type (ADR-0042).

## [0.2.0] - 2026-11-12

**Before you update:** veilq now fills faces in pictures black, and a
picture is refused until the face detector is installed. Run `veilq models
fetch face` once (0.2 MB; in the UI: Settings → Models → Faces in
pictures), or set
`[picture] faces = "keep"` in your policy to leave faces as they are.

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
