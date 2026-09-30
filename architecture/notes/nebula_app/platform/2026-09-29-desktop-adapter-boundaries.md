# Desktop adapters and shared window ownership

## Status

Implemented at the code level; compositor-specific behavior needs native acceptance.

## Context

Several desktop settings were exposed only on Windows. A window, its PTY, and
its persisted settings must keep one owner even when native UI APIs differ.

## Evidence

- `gpui_shell/workspace/windowing.rs` retains the shared window registry.
- `platform/window_visibility.rs` adapts AppKit, X11 and Wayland visibility requests.
- `platform/quick_window.rs` contains existing Win32 geometry primitives.
- `platform/global_shortcut.rs` uses the desktop portal on a real Wayland window.
- `platform/tray_native.rs` owns Unix tray objects and native callbacks.

## Decision

Keep window selection, terminal state and persistence in the existing registry.
Concentrate native handle operations in `platform`; do not add a new trait hierarchy
or duplicate the workspace. A failed native visibility request does not mark a
workspace hidden. Quick-window toggles retain the same PTY.

macOS tray objects stay on the GPUI main thread. Linux uses `tray-icon`'s KSNI
backend, whose DBus work is owned by a worker; no second GTK event loop is added.
Tray callbacks send existing application commands. Snapshot updates coalesce and
shutdown drops native resources and joins the worker.

Settings may be applied before the native shell is initialized. Such updates only
record state until the tray callback owner exists; they must not construct AppKit
menus in a headless workspace or on a test thread.

Wayland global shortcuts use a portal session, including its user authorization
flow, and close the old session on changes or shutdown. Failed authorization is
not retried repeatedly without a settings change. X11 and Windows retain the
existing shortcut registrar. Backend selection comes from the actual window handle.

## Rejected alternatives

- A blanket Unix capability switch with empty adapters would leave dead controls.
- Replacing the application's event loop to support a tray would expand the scope.
- Mutating Wayland surfaces behind the renderer would risk surface lifecycle errors.
- Copying native Win32 geometry rules into the shared workspace would perpetuate
  the coupling responsible for missed platform coverage.

## Consequences

AppKit uses native show/hide, X11 queues map/unmap on the borrowed connection, and
Wayland requests compositor-managed minimization/activation. Wayland cannot promise
arbitrary positioning, Windows-style slide animation, or unconditional foreground
activation through the pinned GPUI API. A tray also depends on the desktop hosting
a status notifier. These platform constraints are not proven away by capability flags.

Provider key input uses the same masked GPUI dialog across platforms and blocks
Copy/Cut actions. Submitted writes retain their settings entity until metadata
completion. This does not imply that widget undo buffers are zeroized. OS credential
stores and their availability remain behind the existing credential adapter.

## Validation

The masked dialog has a real action/clipboard/cancel regression. Existing window
policy and geometry tests remain authoritative. Native compilation and the existing
CI matrix check platform APIs. Graphical tray, shortcut and compositor behavior has
not been manually accepted on macOS/Linux in the Windows development environment.

## Supersedes

None.

## Revisit when

GPUI exposes activation tokens, explicit hide or native external-drag lifecycle
APIs, or a supported desktop requires a different status-notifier backend.
