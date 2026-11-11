# ADR-0031: A click starts veilq without a console window

- **Status:** Accepted (a fix of ADR-0027 and ADR-0029, reported by the owner 2026-11-11)
- **Date:** 2026-11-11
- **Deciders:** project owner
- **Related:** [ADR-0027](0027-a-double-click-opens-the-ui-and-setup-installs-for-the-user.md), [ADR-0029](0029-veilq-shows-its-ui-in-a-window-of-its-own.md)

## Context

veilq.exe is a console program, so that it works in a shell. Started from
the Explorer, the taskbar or the Start menu, Windows gave it a console
window of its own, which ADR-0027 hid with `ShowWindow`. With Windows
Terminal as the default terminal (Windows 11), that window is the
terminal's, and it stays open: a click on the pinned veilq opened a
terminal beside veilq's window.

## Decision

- **The manifest** of veilq.exe sets `consoleAllocationPolicy` to
  `detached`: where Windows knows it, a start from the Explorer, the
  taskbar or the Start menu makes no console at all; a start from a shell
  shares the shell's console as before.
- **Where Windows ignores it** (older versions), the click's veilq has a
  console of its own: it starts itself again as `veilq desktop` (hidden
  from the help) with `CREATE_NO_WINDOW` and ends; the console shows for a
  moment at most.
- A double-click veilq that cannot start says why in a message box, not on
  a console that is not there.

## Alternatives considered

- **A GUI-subsystem program:** no console, but a shell would not wait for
  it and would print nothing of it; the CLI would need a second program
  (`veilqw.exe`).
- **Hiding the console harder:** Windows Terminal owns the window; nothing
  that veilq hides reaches it.

## Consequences

- A fifth Windows call is `unsafe` (`MessageBoxW`), in
  `desktop/windows.rs` with its reason.
- On a Windows that ignores the manifest a console can flash for the
  moment of the first start.

## Enforced by

- `crates/veilq/assets/veilq.manifest`, `crates/veilq/assets/veilq.rc`
- `crates/veilq/src/desktop.rs` (`double_clicked`, `handed_on`,
  `report_error`), `crates/veilq/src/desktop/windows.rs`
  (`console_processes`, `relaunch_without_console`, `error_box`)
- `crates/veilq/src/main.rs` (`desktop`)
