# ADR-0027: A double click opens the UI, and setup installs for the user

- **Status:** Accepted (card in the conversation, points 1 to 6, approved by the owner 2026-11-11); amends [ADR-0008](0008-the-web-ui-is-local-token-bound-and-reads-under-one-root.md) (the UI may write what setup and the model page ask for) and [ADR-0005](0005-models-are-fetched-explicitly-and-pinned-in-the-binary.md) (a model may be fetched from the UI); its app window on Windows superseded by [ADR-0029](0029-veilq-shows-its-ui-in-a-window-of-its-own.md)
- **Date:** 2026-11-11
- **Deciders:** project owner
- **Related:** [ADR-0021](0021-the-ui-token-can-be-kept-in-a-file-across-starts.md), issue #10 (signing)

## Context

veilq is to be used by people who do not use a command line: they
download `veilq.exe` and double-click it. Until now that opened a console
with the help text and closed it. Models were fetched with `veilq models
fetch`, and the UI only read.

## Decision

This ADR records phase 1 of the card (Windows) and the update (point 6);
the user policy, the policy form, folder redaction, macOS's `veilq.app`,
Linux's `.desktop` and "Connect to Claude" follow under the same approval.

- **A double click** (on Windows: the process is alone on its console, so
  the Explorer made it) starts the UI in an app window (Edge or Chrome with
  `--app`, else the default browser) and hides the console. It reads the
  user's home folder; its token is kept in `%LOCALAPPDATA%\veilq\ui-token`.
  `veilq` without arguments in a shell prints the help as before.
- **One instance:** the running UI is written to
  `%LOCALAPPDATA%\veilq\ui.json`; the next double click opens its window
  instead of starting a second server. The window says every 20 seconds
  that it is open; three minutes after it last did, the server ends. The
  settings page has "Quit veilq".
- **Setup** (Settings, opened on the first start until veilq is installed)
  copies the program into `%LOCALAPPDATA%\Programs\veilq` and, as the
  switches say, adds or removes a Start menu entry, a desktop shortcut and
  the user's PATH entry, and registers in "Installed apps" (HKCU), whose
  uninstall runs `veilq uninstall`. No administrator rights. Pinning to the
  taskbar is left to the user: Windows lets no program do it.
- **Models** can be downloaded from the settings page: the same pinned
  files and SHA-256 as `veilq models fetch`, with progress; afterwards the
  engine is built again, without a restart.
- **Update, only when asked:** Settings' "Check for updates" and `veilq
  update --check` ask GitHub's API for the newest release; nothing else is
  sent, and nothing is checked by itself. "Install … and restart" (only in
  a veilq a double click started) and `veilq update` download this
  platform's archive of that release, check it against the release's
  `SHA256SUMS`, move the running program aside (`veilq.old.exe`, removed at
  the next start) and put the new one in its place; the UI starts it and
  ends.
- `veilq.exe` carries an icon (drawn from the favicon by `scripts/icon.py`).

## Alternatives considered

- **An installer (MSI, NSIS):** another artifact to build and sign, and it
  needs administrator rights for the usual places; a per-user copy needs
  none.
- **A native window (Tauri, WebView2):** a second UI stack; the browser's
  app mode looks alike.
- **Ending the server when the window closes:** a browser does not say so
  reliably; the heartbeat is what it can say.

## Consequences

- The UI writes now, behind the same token, `Host` and `Origin` checks:
  the user's shortcuts, PATH and "Installed apps" entry, and model files
  whose checksum the binary pins. Anyone who has the token can do that;
  the token stays in the user's profile.
- The workspace lint goes from `forbid` to `deny` for `unsafe_code`:
  `desktop/windows.rs` allows it for three Windows calls (the console's
  processes, hiding the console, announcing a changed environment), each
  with its reason.
- Unsigned, the downloaded `veilq.exe` gets a SmartScreen warning (#10).
- The update's checksum comes from the same release as the archive: it
  catches a broken download, not a release that was tampered with. Until
  the binaries are signed (#10), the build provenance attestation (`gh
  attestation verify`) is the check for that, and the update does not run
  it.

## Enforced by

- `crates/veilq/src/desktop.rs`, `crates/veilq/src/desktop/windows.rs`
- `crates/veilq/src/ui_setup.rs` (`/api/setup`, `/api/models`, `/api/update`,
  `/api/quit`, `/api/ping`), `crates/veilq/src/fetch.rs` (shared with the
  CLI), `crates/veilq/src/update.rs` (`veilq update`)
- `crates/veilq/src/main.rs` (no command, `uninstall`)
- `ui/src/components/SettingsPane.vue`

## What this ADR does not say

That models can be removed from the UI: a loaded library cannot be deleted
on Windows while veilq runs; `veilq uninstall --models` removes them.
