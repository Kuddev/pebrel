# AI Agent sidebar behind a runtime toggle with two renderer paths

## Status

Decided 2026-10-14 while packaging the `feat/sidebar_tree` sidebar rewrite.

2026-10-17: the "Legacy isolation" decision below (verbatim
`sidebar_legacy.rs` copy) was replaced by the upstream-identical
`sidebar.rs` + `sidebar_tree.rs` split; see
[`2026-10-17-sidebar-upstream-identical-split.md`](2026-10-17-sidebar-upstream-identical-split.md).
All other decisions here stand.
the grouped sidebar ships behind `ai_agent_sidebar` (default off) so the
branch's visual rework is opt-in and `main`'s flat-list behavior remains
reachable without a revert.

## Context

`feat/sidebar_tree` replaced the sidebar renderer in place: the flat tab
list, TABS section header with its 240 ms fold animation, uniform row-height
windowing and drag make-way animation were deleted, and the new grouped
renderer (project groups, SSH host groups, recent AI sessions, tree guides,
two-line entries) became the only path. Product wanted the new style
available but not forced: a settings switch (外观 → 终端外观 → AI Agent
侧边栏) must fully restore the classic behavior when off, and when on the
sidebar badges must show AI-session state only — plain terminals keep no
regular status marks (pane-level failures stay visible).

## Evidence

- `git diff main...feat/sidebar_tree` showed the legacy render path deleted
  from `sidebar.rs`/`tab_scroll.rs` with no feature flag; `resting_activity`
  semantics changed in place at the shared `tab_presentation` call site.
- `python3 scripts/check_architecture.py --base main` was already red
  (workspace.rs 2782 > 2774) before this change, so the restore had to be
  line-neutral or better.
- The grouped sidebar disabled vertical tab-drag activation
  (`tab_drag.rs`, Horizontal only) and the recent-sessions poll gate did not
  know about any mode, so both needed the toggle to restore `main` resource
  and interaction behavior.
- Existing UI tests confirmed section-header `on_click` is unreliable in the
  test environment (`ui_tests.rs` scroll test comment); tests toggle fold
  state through methods instead.

## Decision

- **Single dispatch point**: `render_sidebar` checks
  `config::ai_agent_sidebar(cx)` each frame and delegates to
  `render_sidebar_groups` (branch renderer) or `render_sidebar_legacy`.
  Settings changes reach the existing global-rebuild + notify chain, so no
  new subscription is needed.
- **Legacy isolation**: the restored `main` renderer lives in
  `sidebar_legacy.rs` (~750 lines, copied verbatim from `main`); colliding
  impl members carry a `legacy_` prefix (`legacy_toggle_tabs_section`,
  `legacy_wrap_tabs_scroll_list`, `legacy_remember_tabs_list_bounds`,
  `legacy_tabs_overlay_bar`, `legacy_resting_activity`). Fold animation
  state is packed into `TabsFold { armed, frozen, seq }` on the workspace so
  the restore adds one field, not three.
- **Scrolling**: `tab_scroll.rs` keeps both windowing maths. The uniform
  helpers (`rows_h`/`visible_count`/`reveal_index`/…) are restored verbatim;
  `reveal_active_tab` does both — writes the uniform-window reveal back
  immediately (classic) and sets `tabs_scroll_pending_reveal` (grouped,
  consumed by render). `press/drag/wheel` pick geometry via
  `scrollbar_target` by mode; `clamp_tabs_scroll` stays the grouped coarse
  clamp because both renderers clamp virtually on read.
- **Status source filter at the consumer, not the source**:
  `TerminalView::sidebar_activity()` is shared by sidebar and top tabs, so
  filtering happens in the grouped entry row (`agent_scoped_activity`):
  non-agent tabs fall back to `Idle`, `Failed` (pane death / SSH drop) is
  the only exception kept. `TabPresentation::is_agent_session` is derived
  from the same `AgentKind::parse` that drives the brand headline. Top tabs
  consume the unfiltered field and cannot see the filter by construction.
- **`resting_activity` follows the toggle** at the shared
  `tab_presentation` site: off restores the `main` event semantics
  (current tab shows no completion dot) via `legacy_resting_activity`, on
  keeps the branch's Ready-state semantics.
- **Line budget**: restoring the field plus `mod` declaration costs +4;
  moving `new_tab_insert_index` to `tab_presentation.rs` (its only consumer
  cluster) refunds −14, landing workspace.rs at 2773 ≤ 2774 without raising
  any budget.
- Plain-terminal drag (Vertical) re-arms when the toggle is off; the
  recent-sessions poll gate additionally requires the toggle so the loop
  truly exits in classic mode.

## Rejected alternatives

- Filtering inside `sidebar_activity()` with a sidebar-only flag: the
  entry point is shared with `top_tabs.rs`; a parameter would leak the
  distinction into every caller and invite future misuse.
- Restoring the legacy path inside `sidebar.rs`: the file already carries
  the grouped renderer plus its tests; interleaving two renderers there
  would double review surface and re-trip the line budget.
- A workspace-cached mode field to avoid `cx` reads in dispatchers: stale
  state on toggle; `reveal_active_tab` instead does both behaviors, which
  are idempotent across modes.
- Keeping both fold state styles as three flat fields: three legacy-only
  fields on the workspace struct for one renderer path; one packed struct
  with a doc pointer is cheaper and self-documenting.

## Consequences

- Two renderer paths and two windowing maths coexist until retirement;
  every sidebar change must state which path(s) it touches. Shared pieces
  (`tab_presentation`, header/title-bar renderers, scrollbar drag overlay)
  are deliberately single-copy.
- Switching the toggle mid-session needs no reset: `apply_runtime_settings`
  already calls `reveal_active_tab`, both modes re-clamp their windows
  virtually, and per-mode fold/collapse states survive a round trip.
- The grouped renderer's UI tests now opt in via `enable_agent_sidebar`;
  tests that forget it exercise the classic path by default, which is the
  shipping default.

## Validation

- `cargo test -p nebula-settings` (defaults, boolean spellings, reset
  coverage) and the config/settings-pane getter and switch tests pass.
- `workspace::sidebar*`/`tab_scroll` unit suites cover both
  `resting_activity` semantics, both windowing maths and
  `agent_scoped_activity` (plain states hidden, `Failed` kept, agent
  passthrough).
- `ai_agent_sidebar_toggle_switches_renderers` draws classic rows (fold
  hides them after the freeze clears), then flips the global setting and
  asserts group headers plus the section hairline appear in the same
  window.
- `python3 scripts/check_architecture.py --base main` reports no ERROR;
  i18n contract tests pass with the new en-US/zh-CN keys.
- Known machine-local failures (non-15px UI font in the persisted settings
  file) are unchanged and out of scope.

## Supersedes

None.

## Revisit when

- The toggle is judged stable enough to default on: delete
  `sidebar_legacy.rs`, the legacy windowing block in `tab_scroll.rs`,
  `TabsFold`, the `legacy_resting_activity` branch and the drag-axis gate.
- The recent-sessions gate or the drag axis grows another consumer: extract
  the mode check instead of duplicating the condition.
