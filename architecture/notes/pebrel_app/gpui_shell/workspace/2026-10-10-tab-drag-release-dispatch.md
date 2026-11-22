# Tab drag release dispatch

## Status

Fixed in the PR for #572.

## Context

Tab drag (vertical in the sidebar, horizontal in the top bar) ends on mouse-up:
dock into a terminal half, reorder, merge into another Pebrel window, or tear
out into a new window. On a maximized single-screen Windows window the cursor
physically cannot leave the viewport, so the legacy "cursor left the window
bounds" tear-out never fires. The accepted remedy is Alt-force-tear-out,
priority over dock and cross-window merge. The first real-machine round still
produced no new window, and left-sidebar tabs appeared frozen while dragging.

## Evidence

Three independent layers silently consumed the gesture (diagnostic trace plus
real-machine reproduction):

1. Alt was sampled at release only; users release Alt before the button.
2. `gpui_windows` synthesizes WM_NCMOUSEMOVE as a `MouseMoveEvent` with
   `pressed_button: None` (`handle_nc_mouse_move_msg`), and `update_tab_drag`
   treated any non-Left move as the end of the gesture — windows-edge moves
   cancelled the drag mid-flight.
3. Every hitbox listener, including `capture_any_mouse_up`, self-gates on
   `hitbox.is_hovered` in GPUI dispatch. The drag overlay `.occlude()`s the
   root, so while a drag is active the root capture listener never fires; and a
   release outside the viewport hit-tests nothing, so *all* gated listeners
   (root capture, overlay `on_mouse_up`, sidebar/root `on_mouse_move`) skip.
   The overlay's plain `finish_tab_drag` fallback covered in-window releases
   (dock/reorder only). Releases over another window (merge) or past the window
   edge (tear-out) were therefore dropped: the trace showed `activated` lines
   with no matching release. The source-side merge machinery
   (`cursor_drop_target`, `update_cross_window_drag_target`,
   `drop_tab_to_existing_window`) already existed but was unreachable, because
   during an internal drag the source window holds OS capture and the pinned
   `gpui_windows` does not implement the external-drag handoff
   (`can_start_external_drag` / `start_external_drag` keep their default
   `false`), so other windows never receive drag events.

## Decision

Route every release through one function, `release_tab_drag_at` (branch order:
Alt/latched tear-out, cross-window destination, outside-viewport tear-out,
dock/reorder fallback), reachable through three doors:

- the root `capture_any_mouse_up` listener (in-window releases, unchanged),
- the overlay `on_mouse_up` listener (covers the occluded-root case while the
  drag overlay exists),
- a window-level raw listener registered during paint by a zero-visual
  `gpui::canvas` (`render_tab_drag_global_capture`): its MouseUp handler
  (capture phase, Left) always calls `release_tab_drag_at`, and its MouseMove
  handler acts only when the position is outside the viewport, feeding
  `update_tab_drag` so the cross-window dock preview and the Alt latch keep
  working under OS capture.

All doors are idempotent — the first to run takes the drag state, the rest
no-op. The Alt intent is latched in `TabDrag.force_tear_out` on any move that
observes Alt, so the release no longer depends on modifier sampling at that
instant. NC-shaped moves no longer end active drags; only not-yet-activated
(pending) drags are cancelled by them. Cross-window targeting resolves the
native cursor (GetCursorPos → WindowFromPoint → ScreenToClient) instead of
platform drag/drop, because the pinned GPUI pins internal drag capture to the
source window.

## Rejected alternatives

- `Window::capture_pointer` / `captured_hitbox` ("routed regardless of hit
  testing"): has zero callers in the pinned checkout and requires a `HitboxId`
  the workspace cannot obtain from its element tree.
- Implementing the platform external-drag handoff in `gpui_windows`
  (`can_start_external_drag` and friends): the GPUI baseline is pinned to an
  exact git rev and any patch requires a dedicated branch plus a new exact rev
  — out of scope for this fix and not needed once the release is ungated.

## Consequences

- In-window releases take the exact old path whenever no Alt/latch/cross-window
  destination applies (the fallback equals the previous `finish_tab_drag`
  semantics), so dock and reorder behavior is unchanged.
- The raw listener only exists while `tab_drag.is_some()`, and a release with
  no drag state no-ops.
- Alt (or a latched Alt) is a strict priority over dock merge and cross-window
  merge.
- While dragging over another window, the target shows the existing
  `cross_window_dock` half-area preview; that path needs no new state.

## Validation

- `windowing/transfer_tests.rs` (7 targeted tests): state-machine coverage for
  Alt tear-out, NC-move keep-alive and pending-drag cleanup, plus real-dispatch
  visual tests `alt_drag_survives_nc_move_and_tears_out_via_real_events` and
  `release_outside_source_window_tears_out_without_alt` (the latter fails on
  the pre-fix tree because no gated listener fires outside the viewport).
- Real machine (Windows 11, maximized): tear-out (many), merge onto another
  window (multiple), and in-window fallback all verified by the reporter before
  the diagnostics were removed.
- `scripts/check_architecture.py` exit 0 on the final tree.

## Supersedes

None.

## Revisit when

`gpui_windows` implements the platform external-drag handoff, or GPUI exposes
an ungated pointer-capture API — the raw canvas listener can then yield to the
platform path.
