# Nonblocking logo preparation and first presentation

## Status

Proposed; Windows native launch probes and targeted regression tests passed.

## Context

The initial first-frame change computed final geometry before creating the HWND
and removed duplicate logo work, but still prepared the remaining 15 textures on
the UI thread. Debug startup consequently retained about two seconds of blank
window time. The user requires usable content without waiting for those textures.

The Acrylic-only test package did not contain that separate startup branch.
Combined test packages must identify both changes; material and startup ownership
remain separate from the packaging integration branch.

## Evidence

- Pinned GPUI `App::open_window` builds/draws the initial scene after the Windows
  platform constructor has already shown its HWND. That draw does not present.
- `on_next_frame` callbacks and effects deferred from them run before present;
  they are not completion notifications. Windows has no frame waker, and ordinary
  invalidation of a hidden HWND does not guarantee delivery of its first paint.
- GPUI Windows handles `WM_PAINT` synchronously through its frame callback, which
  presents an initial pending scene even while the window is hidden.
- In isolated Debug launches with the combined material/startup code, first paint
  completed before visibility. The first full screenshots contained the controls;
  sampled native bounds stayed constant through settled content. UI-thread CPU
  from process start to visibility was about 0.52-0.59 seconds. Logo CPU work
  continued on the worker after the UI became usable. Windows' opening animation
  still affects early screen pixels; an HWND visibility event is not a screenshot.

## Decision

- A workspace-owned `LogoLoad` prepares the existing pixel pipeline on the
  background executor. Its UI task holds a weak entity and returns pixels into a
  ready slot. Render consumes ready pixels for tabs and pane headers without
  waiting for decoding. Upstream #241 moved settings Agent icons to static SVGs;
  the former settings-logo synchronization is no longer needed.
- Replacing or dropping a request cancels its sole foreground publishing task
  and signals cooperative cancellation between images. Reset and publication
  run on the same foreground thread; a cancelled publisher cannot apply an old
  worker result. An image already decoding can finish without updating the UI.
- Initial missing brand images use the existing glyph fallback in exactly the
  eventual image width. A DPI transition keeps its prior image while replacement
  is pending, avoiding a flash back to a different glyph. This can briefly resample
  the old DPI texture; cancelled requests cannot replace it afterward.
- Only visible, focused Windows windows defer their initial show. The caller
  leaves silent-start, background-created windows and Quick Terminal unchanged.
- After window creation and deferred material setup, schedule a foreground task.
  Resolve the live HWND in a short GPUI update, leave that App/Window borrow, then
  synchronously send `WM_PAINT`. After its handler returns, activate through GPUI
  to consume pending placement and honor the original focus request.

## Rejected alternatives

- A fixed sleep or next-frame/defer callback cannot prove a frame was presented.
- Raw `ShowWindow` would leave GPUI's pending initial placement unconsumed and
  could reset geometry at later activation.
- Making every window activate would break background and silent-start behavior.
- A synchronous decode before showing merely relocates the startup stall.
- Generation checks duplicate the owned foreground task's cancellation guarantee:
  background workers only produce pixels and never publish into the workspace.
- New image filters, rescaled source assets or persistent caches would change
  visual output or introduce unrelated cache/version/persistence policy.

## Consequences

No new thread pool, timer, per-frame native operation or global logo cache is
introduced. Each window owns at most one publishing request. Cancelled synchronous
image work is bounded by the current image; it does not mutate UI state.

The presentation adapter depends on the pinned GPUI Windows paint path. Its native
message must run outside any App/Window borrow, on the foreground thread. A queued
task resolves a still-live GPUI handle and cannot revive a destroyed workspace.
Windows' normal opening animation remains enabled.

## Validation

- Pixel comparisons at 15px/23px preserve the previous PNG/tint/filter output.
- Replaced/DPI-returning requests cancel their old publishers; dropping a request
  signals cancellation, and a pre-cancelled worker does not decode images.
- Option tests cover visible/hidden and focused/background combinations.
- Geometry and the first-PTY-size/maximize regression remain passing.
- Native launch screenshots/logs distinguish completed first paint, HWND visibility
  and compositor animation; blur-disabled launches use the same presentation path.
- macOS/Linux retain their original show path. Their native appearance is not
  claimed by Windows probes; Quick Terminal interaction requires separate native
  coverage when that path is changed.

## Supersedes

The synchronous-preparation choice in
[the initial first-frame note](2026-09-22-first-frame-startup.md).
Its base-font geometry, zoom/grid ownership and per-window texture sharing remain.

## Revisit when

GPUI changes `WM_PAINT`, frame throttling or presentation ownership, adds a public
post-present show API, or the product needs a shared cross-window texture cache.
