# Backend capability and paint-driven effect cadence

## Status

Implemented on the feature branch; product native CI and physical presentation
are separate validation steps. This change does not implement Metal effects.

## Context

Portable WGSL preparation removed target compilation from the product loader,
but the controller still had a Windows-only compilation condition and the
settings page borrowed background-shader availability. Neither condition states
whether this particular window can prepare and present terminal postprocessing.
The old recurring timer also queried native window state directly.

## Evidence

- Renderer revision `405ba97f946f17e83176b0df908b4fb862f06e51` exposes a per-atlas
  capability through `Window::supports_postprocess_wgsl`. The default is false;
  DirectX implements preparation, while WGPU requires its qualified Vulkan
  backend, a copyable surface format and a live device.
- The renderer's complete source checks, native compiler/source-reuse tests and
  virtual per-window capability test passed in
  [run 37606178237](https://github.com/Kuddev/zed/actions/runs/37606178237).
- At that revision, Wayland's `WaylandWindow::frame` drives GPUI from compositor
  callbacks. X11's `update_refresh_loop` responds to map/visibility changes.
  This is source-path evidence, not physical compositor acceptance.
- `Window::on_request_frame` draws dirty views; an entity notification is frame
  demand, not evidence that a frame has been painted or presented.

## Decision

- Compile the existing controller wherever `shader-background` is enabled.
  Gate its actual creation and preparation on the window's atlas capability.
  Do not infer availability from the OS or from background-shader settings.
- Isolate native activity queries in `platform/effect_activity.rs`. Retain the
  foreground-HWND and visibility/minimization checks on Windows, including the
  distinction between activation messages and the real input desktop foreground.
  Other hosts use GPUI activation; absence of a hidden-state query is not a
  claim of visibility.
- After a permitted effect paint, own at most one 50 ms timer. On expiry it
  clears itself, reconciles activity and requests a redraw if still permitted.
  Only a later paint can schedule the next timer. Stopped platform frames
  therefore cannot sustain an independent effect timer loop.
- Keep per-pane visibility, reduced motion, Always/Focused/Off settings,
  cancellation, source ordering, budgets, resource adoption and retirement.
  Hiding a pane cancels pending preparation and releases its controller owner.
  A compositor merely withholding frames stops cadence; it does not promise
  immediate GPU resource retirement without a visibility notification.
- Settings query the same current-window capability. Unsupported windows disable
  adding, enabling and reloading, but preserve source removal/reordering, reset
  and disabling existing effects. Recheck capability when enabling is confirmed.
- Pin the component and product to the same renderer identity. Validate the
  product's top-level lockfile; do not present the component's pre-existing stale
  standalone lockfile as independently tested.

## Rejected alternatives

- Dropping only the Windows condition, which would expose unsupported backends.
- Reusing wallpaper availability for a different renderer operation.
- Adding a global capability registry or a separate controller for every host.
- Polling focus forever to infer Wayland minimization, which is neither a reliable
  hidden-state observation nor necessary when platform frames govern repainting.
- Treating virtual capability injection as native rendering support.

## Consequences

Qualified Linux Vulkan surfaces can use the same product controller and source
ABI. Unqualified WGPU surfaces and the current Metal implementation remain
unavailable without rewriting saved settings. Activation or resumed platform
frames can resume normal painting and thus the single owned timer.

## Validation

Existing real-control tests cover keyboard enable/reload/removal and stale
picker/confirmation results. Added cases cover disabling/removing on unsupported
windows and capability loss while a confirmation is open. The existing terminal
fixture tests one wake without another paint, animation modes and pane hiding.
Those virtual tests do not execute a native shader or prove visible GPU output.
Native product CI checks the locked graph on the selected real host platforms;
physical Linux compositor pause/resume and product visuals remain distinct.

## Supersedes

Extends [portable source preparation](2026-10-07-portable-source-preparation.md)
at its stated revisit condition. It replaces the Windows-only controller gate,
not the source ABI or ownership contract.

## Revisit when

The renderer adds qualified Metal/other surface support, a cross-platform hidden
state notification, or changes platform frame demand semantics. Recheck the
single-timer lifecycle against that actual implementation before broadening it.
