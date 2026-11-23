# First-frame geometry and repeated logo preparation

Superseded in its synchronous-preparation choice by
[nonblocking first frame](2026-09-22-nonblocking-first-frame.md); geometry remains.
The later upstream #241 integration also replaced the settings-logo synchronization
described below with static SVGs, as recorded in that successor note.

## Status

Proposed; Windows native startup measurements and focused regressions verified.

## Context

A regular window was created at 1080x720 logical pixels. Workspace construction
then measured the configured base font and asynchronously resized the window to
the 116x30 default grid plus chrome. GPUI exposes the native window before that
construction finishes, so synchronous logo preparation made the blank interval
and the later size jump especially visible in Debug builds.

## Evidence

On one Windows machine at 150% DPI, three fresh-config Debug launches spent
3.29-3.35 seconds between visible HWND and first painted controls. The HWND's UI
thread used 3.16-3.20 seconds in that interval; logo preparation used about 3.15
seconds. Disabling blur retained the delay. Desktop Acrylic initialization and
attachment took 8-16 milliseconds in the instrumented material branch.

Five of the ten embedded AI logos have identical source bytes and no tint for
either theme. Their full PNG decode, Lanczos3 resize and alpha-centering work was
performed twice per workspace/DPI. Sharing only those results reduced preparation
from 20 textures to 15 without changing the pixel pipeline.

With the startup change and Desktop Acrylic combined, three Debug launches used
2.16-2.19 seconds of UI-thread CPU during a 2.19-2.27 second visible blank interval.
Every launch had one native bounds value from first visibility through settled
content. These are local measurements, not a Release or cross-machine guarantee.
The observer tracked only its launched process and HWND thread; unrelated system
CPU use was not counted. Pixel polling adds tens of milliseconds of uncertainty.

## Decision

- Keep PNG preparation, theme tint, Lanczos3 filtering, alpha centering and BGRA
  conversion unchanged in `workspace/logos.rs`. Share immutable `Arc<RenderImage>`
  only where the identity catalog confirms an unchanged source and no theme tint.
- Keep textures owned by the workspace and rebuild them at its actual DPI. The
  existing settings-logo synchronization and DPI render block remain authoritative.
- Put the common grid/chrome sizing rule in `windowing/startup_geometry.rs`.
  On Windows, use the platform's primary-monitor DPI query and the existing font
  shaping/rounding code before creating the native window. Clamp to the visible
  work area and retain the 760x540 resize floor unless the display is smaller.
- Measure again with the actual window. Skip an asynchronous resize only when
  sizes differ by less than one device pixel. Failed DPI preflight and unsupported
  platforms retain the prior post-creation sizing policy; a changed DPI can still
  require a corrective resize.
- Use base font metrics for window size and current zoom metrics for PTY grid
  dimensions. Keep Quick Terminal's actual geometry and its animation ownership.
  Do not alter show/focus/silent-start decisions or the PTY startup resize grace.

## Rejected alternatives

- Hiding the window until preparation finishes only conceals the CPU work and
  changes activation/silent-start timing.
- Deferring logo work to workers introduces cancellation, DPI generations and
  placeholder behavior. Avoid that lifecycle change for this bounded improvement.
- Smaller source assets or a different resize filter risk different icon edges.
- A process-global texture cache would need DPI/theme lifetime and eviction rules.
- Guessing display scale as 1.0 on unsupported platforms can create the wrong grid.
- Enabling optimization in the development profile changes build policy and hides
  the cost without removing duplicate preparation. Release already optimizes it.

## Consequences

Debug startup still does synchronous image work and is not instant. The first
regular window is centered using its final size, instead of preserving the corner
of the old temporary rectangle. Small configured fonts retain a usable window
floor, and large work-area insets cannot make preflight sizing overflow that area.
The material implementation is independent: the change builds from upstream main
with Accent and was also exercised with the separate Acrylic branch.

## Validation

- Logo tests compare the omitted theme preparation pixel-for-pixel at 15 and 23
  physical pixels; they also check distinct theme outputs and texture ownership.
- Geometry tests cover the default grid, sidebar width, display caps, small fonts,
  the minimum size, and subpixel rounding at 100/125/150/200% scale.
- Real Windows launches cover ordinary and blur-disabled startup, combined Acrylic
  startup, a 4pt base font and persisted 28px zoom. Native bounds are sampled from
  first visibility and diagnostic logs identify any fallback resize.
- Native macOS/Linux appearance, display changes during creation, and live Quick
  Terminal/hide-to-tray interaction are not claimed by these measurements.

## Supersedes

None.

## Revisit when

GPUI exposes a portable pre-window display scale API; measured Release startup
still warrants asynchronous preparation; or logo source/tint behavior changes.
