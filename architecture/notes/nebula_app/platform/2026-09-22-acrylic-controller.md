# Windows Acrylic controller ownership and compatibility

## Status

Implemented on the GPUI Windows path; local native acceptance on Windows 11
build 26200.9457, x64 Windows App Runtime 1.8 version 8000.946.1701.0.

## Context

AccentPolicy Acrylic leaves a measurable pattern near screen edges. Dark edge
overlays hide that pattern by changing the visual design. DesktopAcrylicController
reduces it, but its default target policy falls back to opaque gray on deactivation.
Pebrel must retain its existing pixel-alpha opacity model and fast opacity slider.

## Evidence

- A local 2560x1600 / 150% DPI comparison used colored bands and an 8 px checker
  behind floating, screen-edge-aligned and maximized GPUI windows. Configuration
  applied before SetTarget alone still went gray. Applying it after SetTarget,
  followed by zero tint/luminosity opacity, preserved the blurred background.
- The production build's clean background ROI matched that successful prototype
  pixel-for-pixel, including active/inactive/reactivated states. It is slightly
  brighter than Accent, not a promise of identical pixels across implementations.
- The original 16 valid alternating prototype measurements isolated the target
  PID/window-thread CPU and PID GPU counters. Window-thread cycles medians rose
  about 5–10%, working set about 3 MiB; CPU time was noisy. Shared DWM cost cannot
  be attributed reliably to this HWND. These are local prototype observations,
  not a product performance guarantee.
- GPUI's pinned Windows platform runs its quit callback after GetMessage exits;
  App shutdown borrows App and its async wait does not pump Windows messages.
- [SetTarget requirements](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/winrt/microsoft.ui.composition.systembackdrops.desktopacryliccontroller.settarget?view=windows-app-sdk-1.8)
  require a DispatcherQueue and host backdrop opt-in. The
  [native dynamic dependency API](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/framework-packages/use-the-dynamic-dependency-api)
  supports an installed Windows App SDK framework on Windows 11 22H2+.

## Decision

- [acrylic.rs](../../../../nebula_app/src/platform/acrylic.rs) owns controller/target
  pairs by GPUI WindowId on the UI thread. One optional compositor/queue is shared
  across material changes; there is no worker, frame callback or focus polling.
- Clear Accent, blur-behind and system backdrop before attaching the lower
  DesktopWindowTarget. GPUI keeps its upper target. Configuration sets input
  active after attachment; tint and luminosity opacity are zero. Window pixel
  alpha continues to supply the user's opacity and theme color.
- Close per-window resources before changing material or destroying the HWND.
  The synchronous app-quit callback closes remaining targets. A guard around
  Application::run drains the owned queue only after run returns, outside the
  App borrow. Its own RoInitialize reference survives GPUI's OleUninitialize.
  An existing external queue is borrowed and never shut down here.
- Keep runtime activation valid until owned resources are released. Queue drain
  failure retains the context until process exit instead of invalidating pending
  callbacks. The drain has a two-second deadline; it is not on the render path.
- Resolve native package-graph APIs from the already loaded OS module. Require
  build 22621+, the 1.8 framework family and minimum tested 8000.946.1701.0. Select
  a matching installed architecture/version; never install or search for DLLs.
  Missing runtime, unsupported API or failed attachment uses existing Accent.
- Use a private, narrow WinAppSDK 1.8 ABI plus existing Windows crate projections.
  No bootstrap DLL, new NuGet build step or bundled runtime enters the installer.
  SDK activation factories are local rather than process-cached interface pointers.
- Keep the existing per-mode/per-window application gate. Failed GPUI updates
  are not cached as successful; opacity changes do not recreate native resources.

## Rejected alternatives

- Edge masks, larger opaque borders and CPU/capture blur change the visual design
  or add recurring work.
- DWMSBT_TRANSIENTWINDOW/default controller policy does not preserve unfocused
  transparency. Reapplying focus policy before SetTarget does not fix that order.
- Bundling the complete runtime exceeds the existing installer budget. Bootstrap
  alone still requires runtime deployment. An opportunistic installed-runtime
  path preserves startup compatibility without introducing an installer action.
- A new composition thread complicates HWND teardown and adds a thread lifetime.
- Pumping a shutdown queue inside on_app_quit can reenter an already borrowed App.

## Consequences

Machines without the supported runtime retain the old edge behavior. Windows 10,
other GPUs, mixed-DPI monitors, accessibility and system transparency-policy
changes need their own native acceptance; this local run does not certify them.
The selected minimum may be widened only with evidence for that runtime version.

## Validation

- `cargo build -p nebula --locked`
- `cargo test -p nebula --bin pebrel platform::acrylic:: -- --include-ignored --test-threads=1`
  exercises missing-package failure, invalid HWND rollback, independent targets,
  repeated close/reattach, full-size roots and actual DispatcherQueue shutdown.
- `cargo test -p nebula --bin pebrel native_acrylic_gpui_material_switches_and_window_lifecycle -- --ignored --test-threads=1`
  opens real GPUI windows, switches all five material modes, creates a second
  window under an unchanged mode, verifies opacity refreshes do not reattach,
  closes one window and quits with the other still alive.
- Production captures cover floating/left/right/maximized and focus transitions;
  isolated processes exited normally and removed their runtime endpoints.
- Architecture gate uses the upstream-synchronized base commit.

## Supersedes

None. Accent remains the compatibility implementation.

## Revisit when

GPUI owns a system-backdrop lifecycle, the supported runtime changes, or portable
runtime distribution can meet the existing size and platform contracts.
