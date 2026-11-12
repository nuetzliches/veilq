# ADR-0039: veilq is offered in the Microsoft Store as MSIX

- **Status:** Accepted (card in the conversation, points 1 to 7, approved by the owner 2026-11-12)
- **Date:** 2026-11-12
- **Deciders:** project owner
- **Related:** [ADR-0027](0027-a-double-click-opens-the-ui-and-setup-installs-for-the-user.md), [ADR-0005](0005-models-are-fetched-explicitly-and-pinned-in-the-binary.md), [ADR-0035](0035-a-release-is-signed-in-its-own-environment-once-switched-on.md), [#10](https://github.com/nuetzliches/veilq/issues/10)

## Context

The downloaded `veilq.exe` is not signed yet (#10): SmartScreen asks once,
and people without a command line stop there. The Microsoft Store signs the
packages it distributes, installs and updates them, and removes them with
everything they wrote. A test of the v0.1.9 release binary in an MSIX
package showed what works as it is and what does not:

- The window (WebView2), the local server, reading the home folder, the
  single instance, the Start menu entry and pinning to the taskbar work; the
  taskbar shows veilq's icon. OCR and PDF load ONNX Runtime and PDFium from
  the model folder.
- `veilq` on the command line and as an MCP server works through an App
  Execution Alias.
- `models fetch` fails: Windows redirects what a packaged program writes
  under `%LOCALAPPDATA%` into the package's folder, and `curl`, which veilq
  starts to download, does not see that redirection (curl exit 23).
- Setup would be offered (`"setup": true`) and update would replace the
  program in the package's folder, which is read-only once installed from
  the Store.

## Decision

1. **Channel:** veilq is offered in the Microsoft Store as an MSIX package
   besides the GitHub release and winget, which stay as they are. For Store
   users the Store's signature takes the place of #10.
2. **In a package, the Store installs:** veilq knows it runs from a package
   (`GetCurrentPackageFamilyName`); setup is not offered, and `install`,
   `uninstall` and `update` (CLI and UI) refuse and name the Store. The UI's
   updates card says that the Store updates veilq.
3. **Data in the package's folder:** in a package, what stands for
   `%LOCALAPPDATA%` is the package's own folder
   (`%LOCALAPPDATA%\Packages\<family>\LocalCache\Local`), named by veilq
   itself, for its own files and the models. Every process veilq starts
   writes there too.
4. **Uninstalling removes everything:** Windows deletes that folder, the
   models included, when the package is removed.
5. **Command line and MCP:** the package's App Execution Alias `veilq.exe`
   puts `veilq` on the PATH; an MCP client's `"command": "veilq"` stays.
6. **The downloads stay:** ONNX Runtime, PDFium and the models are fetched
   as before, pinned with their SHA-256 (ADR-0005). The Store description
   names that dependency at its start (Store policy 10.2.4); they are the
   described function, not a change of it (10.2.2). Bundling the libraries
   would be a card of its own, if certification asks for it.
7. **CI:** the release workflow's job `package-msix` packs the release's
   `veilq.exe` with `scripts/msix.ps1` (the Windows SDK's `makeappx`) into
   `veilq-x86_64-pc-windows-msvc.msix`, a workflow artifact the owner
   submits in Partner Center. The identity comes from Partner Center as the
   repository variables `MSIX_NAME`, `MSIX_PUBLISHER` and
   `MSIX_PUBLISHER_NAME`; without them the package carries test values. The
   version is Cargo's with a fourth part `0`.

## Alternatives considered

- **Bundling ONNX Runtime and PDFium in the package:** a second way to
  get them, and a larger package, for a certification that has not asked.
- **An MSI or EXE submitted by URL (Store policy 10.2.9):** needs our own
  code signing certificate, the open question of #10.
- **Leaving `%LOCALAPPDATA%` to the redirection and downloading in-process
  instead of with `curl`:** a second HTTP stack in veilq for one case; any
  other program veilq starts would meet the same problem.
- **A sideloaded MSIX on the release page:** Windows installs no unsigned
  MSIX; it waits for #10 like `veilq.exe`.

## Consequences

- One more `unsafe` call (`GetCurrentPackageFamilyName`), in
  `crates/veilq/src/package.rs` with its reason.
- A Store user who used `veilq.exe` before has two copies of the models
  until one is removed.
- The Store listing needs a privacy statement URL, screenshots and an age
  rating; the owner keeps them in Partner Center.
- Store updates follow GitHub releases only as fast as each submission is
  certified.

## Enforced by

- `crates/veilq/src/package.rs` (`packaged`, `local_data`, `refuse`)
- `crates/veilq/src/desktop.rs` (`app_dir`, `setup_available`, `install`,
  `uninstall`), `crates/veilq/src/update.rs` (`install_latest`),
  `crates/veilq/src/main.rs` (`update`, `set_local_data`),
  `crates/veilq-core/src/models.rs` (`set_local_data`)
- `crates/veilq/src/ui.rs` (`setup`, `store` in `/api/info`),
  `ui/src/components/UpdatesCard.vue`
- `packaging/msix/AppxManifest.xml`, `scripts/msix.ps1`,
  `.github/workflows/release.yml` (`package-msix`)
