# ADR-0029: veilq shows its UI in a window of its own

- **Status:** Accepted (card in the conversation, points 1 to 3, approved by the owner 2026-11-11); supersedes the app window of [ADR-0027](0027-a-double-click-opens-the-ui-and-setup-installs-for-the-user.md) on Windows
- **Date:** 2026-11-11
- **Deciders:** project owner
- **Related:** [ADR-0027](0027-a-double-click-opens-the-ui-and-setup-installs-for-the-user.md), issue #14 (macOS)

## Context

ADR-0027 opened the UI in Edge's or Chrome's app mode (`--app=<url>`).
That window belongs to the browser's process: pinned to the taskbar it is
the browser, with the browser's icon, and a click on it does not start
veilq. A web app the browser installs (as devstack's dashboard is) would
need a fixed port, a token kept in the browser, and a server that runs
before its window does: a service with a tray icon, for a tool that is
opened now and then.

## Decision

- On Windows a double-click veilq shows the UI in **a window of its own**:
  WebView2 (part of Windows 10 and 11) through `wry`, the window through
  `tao` (both of the Tauri project, MIT/Apache-2.0). The window belongs to
  veilq.exe: the taskbar, Alt+Tab and a pinned button show veilq's icon,
  and a click on the pinned button starts veilq.
- The server runs on a thread of its own; **closing the window ends
  veilq**, and "Quit veilq" closes the window. The heartbeat and its three
  minutes are for the browser's app window only, which stays as the
  fallback where WebView2 is missing.
- A second double click brings the window to the front (`/api/focus`; the
  new process hands its foreground right to the running one).
- WebView2 keeps its profile in `%LOCALAPPDATA%\veilq\webview`, not beside
  the program. A link to another site opens in the default browser.

## Alternatives considered

- **The Tauri framework:** the same WebView2 and window, plus its own
  configuration, IPC, build tool and bundler; veilq's UI talks HTTP to its
  server, which the CLI and the MCP server share. Its bundler alone may
  serve signed installers later (#10, #14).
- **An installed web app (PWA), as devstack:** a fixed port, pairing, a
  service that runs before the window; see Context.
- **Matching the browser's AppUserModelID in the shortcut:** the ID is the
  browser's own and changes with its versions and profiles.

## Consequences

- veilq.exe grows by about 1.3 MB; `wry` and `tao` come in on Windows only.
- A fourth Windows call is `unsafe` (`AllowSetForegroundWindow`), in
  `desktop/windows.rs` with its reason.
- macOS and Linux keep the browser's app window until #14 and #15.

## Enforced by

- `crates/veilq/src/desktop/window.rs`, `crates/veilq/src/desktop.rs`
  (`focus_running`), `crates/veilq/src/desktop/windows.rs`
  (`allow_foreground`)
- `crates/veilq/src/ui.rs` (`WindowHooks`, `/api/focus`, `window` in
  `/api/info`)
