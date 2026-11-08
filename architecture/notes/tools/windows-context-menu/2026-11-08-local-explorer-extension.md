# Local Windows 11 Explorer command integration

## Status

Implemented for local installation verification; not a signed public package.

## Context

The user supplied `E:\PebrelContextMenu`, an already registered development
sparse package exposing a Rust IExplorerCommand DLL. Its title and icon used
hardcoded installation paths, and failed launches were silently ignored. The
user requested integration, a new release build and replacement of `E:\Pebrel`.

## Decision

Keep this host-specific helper in a standalone Cargo workspace under `tools/`,
which is already covered by first-party source budgets. Explorer COM must not
load the application's GUI, settings or terminal dependency graph. Reuse the
existing pinned Windows bindings and supplied package identity/CLSID. Compile
the two existing Explorer label translations from the installer; the Windows
UI language selects them in the separate COM host.

The optional `-ContextMenu` Windows product build compiles/stages the helper
alongside the existing product outputs. Default build/packaging workflows do
not change. The DLL resolves `<installation>/pebrel.exe` from its own installed
`context-menu` subdirectory. It passes folder paths as native process arguments,
reports launch failures, and obtains the host's current folder for background
invocations with no selected items through IObjectWithSite/IFolderView.

Registration is an explicit per-user development sparse-package operation,
with input/identity and post-registration location checks. No signing keys,
certificate store, Developer Mode policy or machine-wide registry is changed.
The unregister operation verifies ownership of the installation directory.
Manifest DLL and image paths are relative to the external installation root
(`context-menu/...`), not the manifest directory. A first registration using
bare filenames returned Status Ok but COM activation failed; the corrected
paths were verified by the registered-class probe.

## Evidence

The supplied package was registered at `E:\PebrelContextMenu\pkg`, version
`1.0.0.0`, with development mode and Status Ok. The existing application exposes
`--working-directory`; no new CLI or application IPC contract is required.

## Rejected alternatives

- Embed the COM server in the GUI executable: independent lifecycle and exports.
- Add the helper to every cross-platform workspace build: Windows Explorer is
  only needed by this explicit local installation path.
- Preserve hardcoded `E:` paths or silently return success after spawn errors:
  relocated installations would show a menu that cannot open the application.
- Claim Store/MSIX/installer support from a development registration: ownership,
  signing and uninstall integration require separate distribution work.

## Consequences

No application runtime dependency or workflow changes. The new standalone
manifest has its own lockfile; the main workspace graph stays unchanged.
The helper persists in its bounded installed directory and a per-user Windows
package registration. Windows owns the COM surrogate lifetime. The existing
uninstaller does not own this optional add-on; unregister it before removing
the application, and re-register explicitly if its payload is replaced.

## Validation

Focused tests exercise relative Unicode installation paths, launch failures
and COM export pointer/class errors. A separate probe checks the real registered
COM title, icon, state and optional folder invocation. Native product release
build and architecture checks are required. On 2026-11-08 the per-user package `1.0.3.0` registered at
`E:\Pebrel\context-menu` with Status Ok. The real COM command returned the
Chinese label and installed executable icon, and both selected-folder and
no-selection background invocations opened the expected Chinese/space path.
The background probe supplied a real Windows shell folder view. A drive-root
invocation also opened `E:\`; the app Runtime API confirmed the actual pane cwd
for each case. Three unit tests, the existing Windows product-builder test,
script syntax, formatting and architecture checks passed. These probes are
not a full Explorer visual acceptance claim.

## Supersedes

The user's hardcoded standalone local helper, without deleting its source.

## Revisit when

The official installer acquires signed sparse-package ownership and uninstaller
cleanup, or the upstream project supplies its own modern Explorer integration.
