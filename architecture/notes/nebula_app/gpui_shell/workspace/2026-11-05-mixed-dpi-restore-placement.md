# Mixed-DPI restore placement

## Status

Proposed; native Windows reproduction measured, CI-native regressions added.

## Context

A regular window restored on a mixed-DPI desktop could open half on each
display. The saved session stored a logical width/height, a physical client
origin and the display UUID. The startup geometry selected that display,
clamped the logical bounds into its work area, and passed the result to
`WindowOptions`.

The pinned GPUI Windows backend converts those logical bounds with
`GetDpiForWindow` taken immediately after `CreateWindowExW`, which is the DPI of
the monitor that created the HWND, not the DPI of the target display. Windows
then moves the window and delivers `WM_DPICHANGED`; the backend applies the
system-suggested rectangle with `SetWindowPos`, scaling the physical size by the
scale ratio while keeping the top-left. The origin therefore stayed in
creation-scale pixels, and on a display with a larger scale the window grew past
the bottom of its monitor onto the neighbour.

## Evidence

Local reproduction on Windows 11 with a 1920x1200 primary display at 125% and a
3840x2160 display at 200% whose physical origin is (-1033, -2160):

- Saved state: logical 1332x765, physical client origin (-274, -1050), display
  UUID of the 200% monitor.
- The startup geometry computed the intended logical bounds correctly: origin
  (-137, -525), which is (-274, -1050) physical at 200%, and the logical size
  fits the display work area.
- Measured native outer rectangle after activation: (-180, -960) to
  (2513, 584), 2693x1544 at 192 DPI. The bottom 584 physical pixels sat on the
  primary display.
- The window's own autosave recorded the same mis-scaled origin (-171, -956),
  confirming that the backend, not the session math, moved the rectangle.

A per-monitor-v2 aware probe (`SetProcessDpiAwarenessContext`) measured the
rectangle; the launching PowerShell and probe used the same hidden-window paint
path as normal startup, and the launch had to stay unelevated because elevated
launches deliberately skip session restore.

## Decision

After the first ordinary restored window becomes active, re-derive its native
rectangle from the saved geometry instead of trusting the backend conversion:

- Client origin is the saved physical position with the target display's
  work-area offset removed, mirroring `workspace_position` used by the restore
  path. Client size is the saved logical size times the target display's
  effective DPI, rounded once.
- The frame insets (outer minus client, including invisible DWM borders) are
  measured on the live window with `GetWindowRect`, `GetClientRect` and
  `ClientToScreen`, then added to the client rectangle.
- The outer rectangle is clamped fully into that display's physical work area
  with `SetWindowPos(SWP_NOACTIVATE | SWP_NOZORDER)`. A rectangle that already
  matches is not touched, and a zoomed window is skipped.
- The correction runs on the first active frame, deferred out of the render
  pass. GPUI applies its pending placement inside the activation task, so a
  builder-time `SetWindowPos` would be overwritten, and resizing natively during
  render would re-enter the backend's size callbacks.
- It applies only to the first ordinary `RestoreOrDefault` window with a saved
  position, non-maximized, and no explicit Lua position or grid. Hidden startup
  keeps its one-shot pending flag until the tray shows and activates the window.

Saving the corrected rectangle then breaks the previous self-perpetuating state:
a straddling rectangle is no longer persisted, because the window is placed
fully inside one work area before the next session checkpoint.

## Rejected alternatives

- Patching `third_party/winit` or the pinned GPUI: the Windows backend is
  upstream code and the fix must work with the current pin.
- Compensating in `regular_options` by pre-dividing the origin with the creation
  scale: that scale is unknown before the window exists, and
  `check_given_bounds` still validates the logical bounds against the target
  display.
- Waiting for a later user interaction or resize to correct the window: leaves
  the reported first-screen defect visible.
- Changing the saved-size convention to physical pixels: would break older
  sessions and the logical-size contract used by the grid fallback.

## Consequences

Restored windows are placed with the target display's physical scale, so the
saved logical size is preserved instead of being interpreted through the creation
monitor's DPI. The correction can move a window that the backend had already
shown at a wrong position; on the reproduction the jump is limited to the first
frames while the window is being presented. Windows without a top or left
taskbar keep their exact round-trip because the work-area offset is zero.
Maximized restores, configured geometry and later ordinary windows keep their
existing behavior.

## Validation

`restored_window` unit tests cover the measured 125%/200% rectangle including
the bottom clamp, work-area offsets from top/left taskbars, an oversized saved
rectangle, and a fully visible rectangle that must not move. Native placement
continues to be exercised by the existing Windows geometry test. Formatting,
workspace tests, native tests and the Windows package build run in GitHub
Actions; there is no local Rust toolchain, so local `cargo` runs are not part of
this change's evidence. A live Windows run of the packaged build is recommended
before merging upstream.

## Supersedes

Extends [ordinary-window startup geometry](2026-10-03-ordinary-window-geometry.md);
that note's save/restore convention and ownership rules still hold.

## Revisit when

GPUI replaces its creation-DPI conversion with a target-display-aware placement,
or when a platform-independent window placement API becomes available upstream.
