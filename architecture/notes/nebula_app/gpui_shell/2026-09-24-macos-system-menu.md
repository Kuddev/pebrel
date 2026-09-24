# macOS system menu and close shortcuts

## Status

Accepted.

## Context

The GPUI workspace already closes terminal tabs with `cmd-w`, while native window
close must preserve its document, session, and busy-process checks. macOS users
also expect application quit and common editing commands in the system menu.

## Evidence

- [`keyboard_bindings.rs`](../../../../nebula_app/src/gpui_shell/workspace/keyboard_bindings.rs)
  registered `cmd-w` for `CloseActiveTerminal`.
- [`closing.rs`](../../../../nebula_app/src/gpui_shell/workspace/closing.rs)
  owns the existing window-close checks and persistence path.
- [`shutdown.rs`](../../../../nebula_app/src/gpui_shell/workspace/windowing/shutdown.rs)
  owns the save and approval steps before quitting.

## Decision

Use GPUI's native application menus. `cmd-q` calls the shared graceful quit path;
`cmd-w` invokes the workspace close checks; terminal close moves to `cmd-shift-w`.
The app menu exposes About, Settings, Services, Hide, Hide Others, and Quit, with
File, Edit, View, and Window menus alongside it.

## Rejected alternatives

- Keep `cmd-w` for terminal close: conflicts with the expected system window-close
  shortcut.
- Call `remove_window()` directly from the menu: bypasses the workspace close
  checks and session persistence.
- Call `cx.quit()` directly: bypasses the shared quit approval and save path.

## Consequences

Window-menu close and the native close button share the same close behavior.
The menu is rebuilt when the selected UI language changes. No dependency or saved
format changes are introduced.

## Validation

English and Simplified Chinese catalog IDs were compared and are aligned; both
catalogs parse as JSON; `git diff --check` passes. `cargo check` could not run
because `cargo` is unavailable in the current environment. Native macOS menu
behavior has not been exercised in a running app.

## Supersedes

None.

## Revisit when

The GPUI menu API or workspace shutdown and close flows change.
