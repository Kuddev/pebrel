# Spinner frames and terminal render scope

## Status

Implemented and validated locally. Native platform CI is required before merge.

## Context

An Agent running badge needs animation while the user watches a terminal. Its
phase belongs to the workspace, so advancing it notifies that workspace. Before
this change, both the single/zoomed pane and split pane renderers embedded plain
terminal entities. A workspace redraw therefore rendered and painted terminal
subtrees even when only the badge phase changed.

This matters because terminal paint captures the visible grid, resolves colors,
and prepares text and overlays. Reducing the badge's drawing primitives alone
does not remove that terminal work. Conversely, freezing every unfocused window
would remove useful progress feedback while the user works in another window.

## Evidence

- [Workspace pane embedding](../../../../../nebula_app/src/gpui_shell/workspace.rs)
  had no terminal cache boundary. The actual
  [terminal element](../../../../../nebula_app/src/gpui_shell/terminal/element.rs)
  calls `RenderSnapshot::capture` from its paint path. This identifies the
  redundant work by source inspection; it is not a Windows performance profile.
- The pinned [GPUI view implementation](https://github.com/Kuddev/zed/blob/1e716d16ec54b361499931b4278903cdcaae8949/crates/gpui/src/view.rs)
  provides `Entity::cached` with retained prepaint and paint ranges. It checks
  entity invalidation, bounds, content mask, inherited text style and whole-window
  refresh before reusing a subtree.
- The old badge assembled its ring from overlapping, individually colored
  circles on each paint. GPUI's
  [SVG rendering parameters](https://github.com/Kuddev/zed/blob/1e716d16ec54b361499931b4278903cdcaae8949/crates/gpui/src/svg_renderer.rs)
  use asset path and device-pixel size as the cache key; rotation is supplied
  separately to `Window::paint_svg`.
- Constructing a Running tab item does not establish visibility: its status can
  be clipped by a folded section, a hidden sidebar or the top-tab scroll viewport.
- The real-layout scroll regression exposed that top-tab rows used `occlude()`.
  GPUI's [hit test](https://github.com/Kuddev/zed/blob/1e716d16ec54b361499931b4278903cdcaae8949/crates/gpui/src/window.rs)
  then excluded both the ancestor scroller and viewport: a wheel over a gap
  worked, while a wheel over a tab did not. The native title-bar control lookup
  also reads the full hit list, so preserving ordinary hover blocking alone is
  insufficient to keep a tab from becoming a window drag target.

## Decision

Use the shared `terminal_surface` adapter in
[`terminal_activity`](../../../../../nebula_app/src/gpui_shell/workspace/terminal_activity.rs)
for both pane embedding paths. Its definite full-size style allows GPUI's native
entity cache to reuse the terminal subtree on workspace-only invalidations.
The terminal's own notifications, changed layout bounds, inherited text style,
content mask and window refresh retain their existing invalidation authority.
There is no additional terminal snapshot store or process-wide cache registry.

Output, input, cursor and selection continue to update the terminal entity.
The existing [hidden-output policy](../terminal/2026-09-19-hidden-output-invalidation.md)
already defers notifications while hidden and refreshes a pane when it becomes
visible again, including when its output stopped while hidden. Caching relies on
that contract rather than creating another visibility owner.

Reset spinner visibility at the start of workspace render. Both status wrappers
use `invisible()` on hover; GPUI's `Visibility::Hidden` path skips descendant paint
entirely. The remaining badge paint marks it visible and arms one next-frame
callback only when it intersects the content mask. Opacity alone would still
invoke paint and is not a substitute for these Hidden wrappers. A workspace-owned
pending flag deduplicates demand from multiple visible badges. The callback
holds a weak workspace reference, clears the pending flag, checks visibility
again, advances the shared phase and notifies the workspace. Only a subsequent
visible paint renews the chain. A callback already queued when a badge disappears
may run once without renewing; dropping the workspace leaves no strong owner.

[`agents`](../../../../../nebula_app/src/gpui_shell/workspace/agents.rs) samples the
GPUI background executor's clock for a one-second revolution. Visible unfocused
windows continue animating. Explicitly hidden or minimized windows stop under
the existing workspace/platform checks; keyboard focus is not a visibility test.

[`sidebar`](../../../../../nebula_app/src/gpui_shell/workspace/sidebar.rs) paints a
fixed ring SVG and a fixed arc SVG, shared with top tabs. Their masks reuse the
GPUI atlas at a given device-pixel size; phase changes only the arc transform.
Both assets and the rotation center use one pixel-snapped physical square.
Snapping width and height independently could produce a 16-by-17-pixel box while
the SVG mask scales uniformly; one snapped side preserves the shared center at
fractional scaling. Existing semantic colors are composed
against the tab background before painting the overlapping layers.

Top-tab rows use `block_mouse_except_scroll()` so a wheel reaches their scroller.
The viewport itself uses `occlude()` to retain its own scroll hit while excluding
the enclosing title-bar drag hit. GPUI's inner horizontal scroller is the sole
wheel-delta owner; the viewport only stops further event propagation. Applying
the delta again in the workspace would count each wheel event twice. This adopts
GPUI's standard nonzero-X preference, Y-to-X fallback and native line-height
conversion instead of the former stronger-axis rule and fixed line multiplier.
Tabs remain direct scroller children because `ScrollHandle::scroll_to_item`
indexes those children when revealing the newly activated tab.

## Rejected alternatives

- Optimize only the small ring: terminal capture and text preparation would
  still run on workspace spinner frames.
- Add a separate terminal bitmap or snapshot cache: duplicates GPUI ownership
  and adds another invalidation/resource-lifetime contract.
- Stop animation whenever the window loses focus: visible background tasks
  would appear frozen during normal work in another application.
- Schedule from the existence of a Running tab: clipped or folded badges would
  keep requesting frames without presenting progress feedback.
- Start one timer per badge: multiple tabs would create parallel wakeup chains
  instead of sharing the existing window frame source.
- Generate an SVG path or raster image for each angle: makes phase part of asset
  creation rather than reusing two masks with a transform.
- Wrap all tabs in one wheel-handling child: destroys the per-tab child indices
  used by `ScrollHandle` for automatic reveal.

## Consequences

Workspace chrome still renders during animation, while unchanged terminal
subtrees can reuse their scene ranges. Output and interactive changes still
incur the terminal work they require. GPUI remains responsible for retained
view data, atlas storage, eviction and platform presentation.

This change does not establish Windows partial presentation or dirty-rectangle
rendering. It makes no numerical CPU, GPU, frame-rate or memory guarantee.
Native measurements must distinguish terminal capture, chrome work, scene
submission and swap-chain presentation before attributing a total saving.

## Validation

All six real-layout regressions below pass on Linux ARM64. Thirteen existing
settings-navigation, top-tab geometry and title-bar control regressions also
pass. The production build passes
`cargo check --locked -p nebula --bin pebrel --features gpui-shell --tests`.
Workspace formatting, `git diff --check` and
`python3 scripts/check_architecture.py --base 2085557d` pass.

After integration with OSC 7501, the same nineteen layout/control regressions
and 145 terminal-view regressions pass (four existing ignored cases). The
production check, formatting and architecture check against the integrated
protocol commit also pass.

The following tests in `workspace::terminal_activity::tests` use an actual
`Root` containing `NebulaWorkspace`. A test-only, per-App
[snapshot probe](../../../../../nebula_app/src/gpui_shell/terminal/element/snapshot_probe.rs)
records captures consumed by real terminal paint, isolated by entity identity.

| Test | Protected behavior |
| --- | --- |
| `spinner_frames_reuse_single_terminal_snapshot` | Delivered spinner callbacks advance phase without another terminal capture. |
| `spinner_frames_reuse_split_and_zoomed_terminal_snapshots` | Both split and zoomed embedding paths reuse paint; zoom updates grid geometry. |
| `cached_terminal_updates_output_resize_cursor_input_and_selection` | Output and resize refresh actual snapshots; cursor movement/blink, dispatched input and mouse selection invalidate correctly. |
| `sidebar_spinner_follows_real_collapse_and_window_focus` | Real hover, TABS clicks and sidebar actions stop/restart demand; a visible deactivated window keeps animating. |
| `top_tab_spinners_share_frames_and_stop_when_running_tab_scrolls_out` | Multiple Running tabs share one callback; horizontal/vertical wheels over real tabs each apply once, hover/scroll hide and reveal the only Running badge, and keyboard activation reveals the indexed first/last tab. |
| `rendered_tabs_splits_zoom_and_settings_restore_only_presented_panes` | Existing output-visibility transitions remain covered across tabs, splits, zoom, settings and explicit hiding. |

These regressions exercise layout and event dispatch, not a terminal entity
kept outside the layout tree. They do not replace native Windows visual or
performance measurements.

## Supersedes

None.

## Revisit when

The GPUI cache contract changes, a terminal presentation mutation stops emitting
its own notification, the workspace gains new simultaneous/occluded surfaces,
or native traces show chrome or platform presentation dominating after capture
reuse. Extend the real-layout regressions with each new invalidation path.
