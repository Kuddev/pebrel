# Linux desktop settings follow the native window lifecycle

## Status

Implemented candidate; native X11 and Wayland acceptance is required before merge.

## Context

Issues [#629](https://github.com/Kuddev/pebrel/issues/629) and
[#624](https://github.com/Kuddev/pebrel/issues/624) report visible silent startup,
closing instead of retaining sessions, unavailable quick-terminal shortcuts and
separate Dock entries on Linux. The settings were persisted and platform adapters
existed, but several consumers bypassed their ownership boundaries.

## Evidence

- The pinned component's Linux titlebar defaults to `Window::remove_window`,
  bypassing the workspace's system-close callback and its session/document guards.
- GPUI called X11 `map_window` regardless of `WindowOptions.show`; Wayland did not
  gate initial buffer presentation on that option.
- Portal activation was reduced to `()`, losing the compositor's authorization
  token before choosing a window.
- Native app IDs (`pebrel` and `pebrel-quick-terminal`) did not match the packaged
  `.desktop` IDs; their X11 `StartupWMClass` was a third identity.
- X11's checked map request can buffer MapNotify. GPUI's existing deferred event
  drain is needed to restart refresh for a restored window.

## Decision

Route the client titlebar through the same workspace close operation as native
close requests. Keep the window, terminal views, PTYs and persistence ownership.

Keep native presentation in the existing fixed GPUI fork. The patch at
`1e716d16ec54b361499931b4278903cdcaae8949` honors initial visibility, reuses the
X11 map/drain path, and accepts a portal activation token. Wayland performs its
initial configure handshake while hidden but submits no window buffer until shown;
the first visible frame consumes a queued token once. A forced frame restores a
scene whose earlier hidden presentation was intentionally skipped.

The component fork at `78e1d0c48867f69fec297d9016f48b0ce1b8c146` retains the
current reader component APIs and aligns its five framework pins with that revision.
Updating only the application's pins
leaves two framework sources and fails locked resolution. Cargo regenerates the
lockfile; locked metadata contains one GPUI identity, with no unrelated package
version changes.

The app adapter uses this interface instead of manipulating the framework's XCB
connection directly. An already mapped Wayland window retains the previously
documented compositor-minimization behavior. No surface is unmapped behind WGPU.

Preserve portal tokens through the existing bounded event queue and consume them
only when the quick-terminal operation shows a window. Registration errors produce
a visible warning without repeatedly prompting for desktop authorization.

Both window roles use the packaged Linux desktop identity. The executable's own
installation prefix identifies a Preview desktop entry; a normal/source build uses
the stable ID. X11 WM_CLASS and the corresponding desktop entry agree as well.

## Rejected alternatives

- Only save or reapply the switches: their persisted values already reached the
  native boundaries and cannot repair a bypassed close callback.
- Delay hiding until after the first visible frame: this still flashes a window
  during a requested silent launch.
- Detach a null Wayland buffer from application code: unmapping resets toplevel
  attributes and requires another configure handshake. It would require a broader
  renderer lifecycle change than the reported startup and residency defects.
- Request another activation token using an old mouse serial: it discards the
  authorization already supplied by the global-shortcut portal.
- Rename compatibility protocols or third-party dependencies to satisfy branding:
  those identifiers have functional and attribution responsibilities.

## Consequences

Wayland activation and minimization remain compositor decisions; global shortcuts
require a supporting desktop portal and user authorization. The API does not claim
arbitrary positioning, taskbar removal or guaranteed foreground access. Missing
portal support is reported, not treated as successful shortcut registration.

The default native test binary contains an opt-in desktop fixture. The Linux CI
job runs it with Xvfb and Weston, keeping the existing native matrix and compilation
cache. It clicks the actual titlebar, checks terminal-view identity across closing
and restoration, and checks X11 map state. Wayland protocol evidence checks that
the actual surface has no buffer before the first show and does present afterward.

Native window destruction releases its input handler synchronously before queuing
platform close callbacks. Otherwise a final-window quit can retain the handler's
strong terminal-view entity while the native event loop has already stopped.

## Validation

Targeted checks cover desktop-file identity and the native lifecycle above, in
addition to the existing architecture, localization and platform contracts. Native
CI results must be recorded before acceptance. This does not establish manual
CachyOS/GNOME/KDE tray-host or portal-authorization acceptance.

## Supersedes

The initial visibility and direct-XCB adaptation described by
[desktop adapter boundaries](2026-09-29-desktop-adapter-boundaries.md).
Its shared workspace ownership and Wayland capability limits remain applicable.

## Revisit when

The upstream framework exposes equivalent verified APIs, or a concrete compositor
requirement justifies full Wayland unmap/remap lifecycle support.
