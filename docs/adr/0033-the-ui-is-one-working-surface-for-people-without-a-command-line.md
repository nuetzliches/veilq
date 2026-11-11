# ADR-0033: The UI is one working surface for people without a command line

- **Status:** Accepted (card approved by the owner, 2026-11-11, in the pull request)
- **Date:** 2026-11-11
- **Deciders:** project owner
- **Related:** [ADR-0008](0008-the-web-ui-is-local-token-bound-and-reads-under-one-root.md), [ADR-0027](0027-a-double-click-opens-the-ui-and-setup-installs-for-the-user.md), [ADR-0028](0028-a-user-policy-edited-in-the-ui-and-folders-redacted-whole.md), [ADR-0032](0032-the-ui-shows-at-once-and-a-run-waits-for-the-models.md), #18, #34, #35, #36, #37

## Context

Since ADR-0027 a double click opens the UI, so most of its users have never
seen veilq's command line. The UI was still laid out for them: a sidebar
with four tabs (Text, Upload, File, Folder) to pick before anything could
happen, a top bar with `root`, `policy`, `action` and `version`, a findings
table always taking a third of the height, and Settings grown out of the
install page. A user had to learn that text goes into one tab and a
screenshot into another, and that the File tab only reads under a folder
named elsewhere.

The issues ahead put more into the UI: marking a gap and the steps that
check a redaction (#18, #34), an organisation's server (#34, #35), a
reader's reports (#36) and receipts with numbered findings (#37). The
layout has to leave a place for them.

## Decision

- **One input for everything.** The left pane is a text editor with its
  hint as an overlay that goes away at the first character; a file dropped
  anywhere on the window, a pasted file or screenshot and **Open → Open a
  file …** redact that file. After a run, a click on the text edits it
  again; **New** empties the input. Text is redacted with a button (or
  Ctrl+Enter), not as it is typed.
- **Tools behind a menu.** **Open** holds "Open a file …" (the system's file
  dialog) and "Redact a folder …" (the folder run of ADR-0028 as a dialog).
  The browser of files under the root moved to Settings → Advanced.
- **The root chooses itself.** A folder to redact outside the folder veilq
  reads becomes that folder, as choosing it in Settings does. The boundary
  of ADR-0008 stays: the server still reads under one root, and only the
  user's own choice in this window moves it.
- **The output says what to do next.** "Replace with" (the action, labelled
  in words: Placeholder, Made-up values, Remove, Pseudonym) sits above the
  output; Copy and Save … below it. "Please check" is a banner over the
  output. The findings are a panel beside the output, opened from "n
  findings", each **numbered in report order**, the same number in the
  tooltips of both panes.
- **A model is offered where it is missing.** A refusal or review reason
  that names `veilq models fetch <name>` shows a download button for that
  model in place; once the engine uses it, the source runs again. Until
  then the run stays refused (fail closed).
- **Settings is a page of its own** with General (language, updates,
  quit), Detection (rules, models) and Advanced (root, file browser,
  install, about). The first start shows a **welcome dialog** (install,
  models) instead of opening Settings.
- **Two languages.** The UI speaks German where the system's first
  language is German and English otherwise; Settings → General overrides it
  for this browser profile. Messages of the engine and the server stay
  English. Entity types are shown with a name (`PERSON` → Name), a type of
  a user's own pattern as written.

## Alternatives considered

- **Keep the tabs, polish them:** cheaper, but a user still chooses a mode
  before doing anything, and Text and Upload stay two places for one act.
- **Redact text as it is typed:** immediate, but a long text runs the
  engine on every pause, and NER makes that slow; a button is predictable.
- **Keep the root explicit before a folder run:** clearer about the
  boundary, but the folder run was unusable without a detour through
  Settings, and the choice in that dialog is the same user making the same
  choice.
- **A vue-i18n dependency:** more features (plural rules, lazy loading)
  than two catalogues need; a typed key list and a test that both
  catalogues have the same keys and placeholders are enough.

## Consequences

- Screenshots in the README and on the site show the old layout until
  they are taken again.
- The panel beside the output is where checking a redaction goes (#18,
  #34): below the findings, its steps ordered by how far data leaves the
  machine (marking and local rules first; the user's agent, the
  organisation's server and an upstream issue later, each switched on on
  its own). A reader's reports (#36) can go beside the findings in the same
  panel.
- Settings has room for a section per later concern: an organisation's
  server and its rules (#34, #35), receipts and keys (#37).
- The finding numbers are positions in one report; a receipt (#37) that
  numbers findings should use the same order, so a number means the same
  in the UI and in the receipt.
- Two catalogues to keep in step; the test fails when a key or a
  placeholder is missing in one of them.

## Enforced by

- `ui/src/App.vue` (the surface, the menu, drop and paste)
- `ui/src/components/FindingsPanel.vue`, `MissingModels.vue`,
  `SettingsPage.vue`, `WelcomeDialog.vue`, `FolderRun.vue` (the root that
  chooses itself)
- `ui/src/lib/i18n.ts`, `ui/src/lib/messages/en.ts`, `de.ts`,
  `ui/src/lib/labels.ts`
- Test: `ui/src/lib/i18n.test.ts`

## What this ADR does not say

- Nothing about marking a gap or the case store: those need their own card
  (#34).
- The server's API and the root's boundary are unchanged.
