# AI Agent sidebar behind a runtime toggle with two renderer paths

## Status

Proposed implementation; maintainer corrections applied on 2026-10-19.
Visual acceptance and native verification remain pending.

## Context

The grouped sidebar (project groups, SSH host groups, recent AI
sessions, tree guides, two-line entries) rewrites the sidebar's visual
language. Product wanted it available but not forced: a settings switch
(外观 → 终端外观 → AI Agent 侧边栏) must fully restore the classic
flat-list behavior when off, and when on the sidebar badges show
AI-session state only — plain terminals keep no regular status marks
(pane-level failures stay visible).

The structural constraint: the classic renderer must remain exactly the
upstream `sidebar.rs` so that upstream changes to it merge without
conflict and the OFF path tracks upstream for free.

## Evidence

- `git diff` of the sidebar rewrite showed the flat renderer replaced
  in place with no feature flag and `resting_activity` semantics changed
  at the shared `tab_presentation` call site — product required the
  classic behavior to remain reachable.
- The flat renderer's slot/title-bar helpers were byte-identical to
  upstream's, so an upstream-identical `sidebar.rs` plus a tree module
  was mechanically extractable; the tree renderer consumes only
  `pub(super)` helpers plus one const from `sidebar.rs`.
- Existing UI tests confirmed section-header `on_click` is unreliable in
  the test environment; tests toggle fold state through methods or field
  writes instead.

## Decision

- **File split**: `sidebar.rs` stays upstream verbatim except a minimal,
  stable delta — a `..` in the `TabPresentation` destructure (this
  change adds `headline`/`is_agent_session` fields the classic path
  does not consume), two internal call sites inside
  `render_sidebar_slot` rerouted to `render_sidebar_dispatch`, and that
  appended dispatch impl (~24 added lines, all but the two call lines
  pure additions). All grouped rendering lives in `sidebar_tree.rs`
  (row tree, section/host/group headers, status labels, agent scoping,
  the Ready-state resting semantics as `tree_resting_activity`).
- **Single dispatch point**: `render_sidebar_dispatch` checks
  `config::ai_agent_sidebar(cx)` each frame and delegates to
  `render_sidebar_tree` or upstream's `render_sidebar`. Settings changes
  reach the existing global-rebuild + notify chain; no new
  subscription.
- **Scrolling**: `tab_scroll.rs` keeps both windowing maths. The
  uniform helpers (`rows_h`/`visible_count`/`reveal_index`/…) stay
  verbatim from the classic path; `reveal_active_tab` does both — it
  writes the uniform-window reveal back immediately (classic) and sets
  `tabs_scroll_pending_reveal` (grouped, consumed by render).
  `press/drag/wheel` pick geometry via `scrollbar_target` by mode;
  `clamp_tabs_scroll` stays the grouped coarse clamp because both
  renderers clamp virtually on read.
- **Status source filter at the consumer, not the source**:
  `TerminalView::sidebar_activity()` is shared by sidebar and top tabs,
  so filtering happens in the grouped entry row
  (`agent_scoped_activity`): non-agent tabs fall back to `Idle`,
  `Failed` (pane death / SSH drop) is the only exception kept.
  `TabPresentation::is_agent_session` derives from the same
  `AgentKind::parse` that drives the brand headline. Top tabs consume
  the unfiltered field and cannot see the filter by construction.
- **`resting_activity` follows the toggle** at the shared
  `tab_presentation` site: off keeps the classic event semantics (the
  current tab shows no completion dot, `sidebar::resting_activity`
  unchanged); on switches to the Ready-state semantics
  (`sidebar_tree::tree_resting_activity`: Done/Completed read as
  "turn finished, ready for the next" and stay visible on the active
  tab).
- Plain-terminal drag (Vertical) re-arms when the toggle is off; the
  recent-sessions poll gate additionally requires the toggle so the
  loop truly exits in classic mode.

## Rejected alternatives

- Filtering inside `sidebar_activity()` with a sidebar-only flag: the
  entry point is shared with `top_tabs.rs`; a parameter would leak the
  distinction into every caller and invite future misuse.
- Interleaving the second renderer inside `sidebar.rs`: doubles the
  review surface of a file upstream actively evolves and re-trips the
  per-file line budget.
- A workspace-cached mode field to avoid `cx` reads in the dispatcher:
  stale state on toggle; `reveal_active_tab` instead does both
  behaviors, which are idempotent across modes.
- Dispatching at the `render.rs` call site to keep `sidebar.rs` 100 %
  byte-identical: `render_sidebar_slot` and the title bar call the
  renderer internally, so tree mode would lose the fold slot chrome
  unless those call sites route through the dispatch anyway.

- Restore scrollbar visibility preferences and their existing behavior tests;
  grouping does not change terminal scrollbar geometry or hover behavior.
- Local cwd/HEAD discovery is connected to TitleChanged and setting refresh.
  A window owns one background probe, coalescing pending cwd requests and
  retaining results only for currently focused pane directories. Switching off
  clears pending work and results; rendering never reads the filesystem.
- Shell Git reporting is opt-in at session creation. Existing shells retain
  their environment until reopened/reconnected; the sidebar renderer changes
  immediately. SSH-specific new-tab inheritance is also gated by this mode.

## Consequences

- Two renderer paths and two windowing maths coexist until retirement;
  every sidebar change must state which path(s) it touches. Shared
  pieces (`tab_presentation`, slot/title-bar renderers, scrollbar drag
  overlay) are deliberately single-copy.
- Upstream changes to the classic sidebar merge as textual no-ops
  except the two dispatch call lines; classic-mode behavior is
  upstream's, including future changes we do not review at merge time.
- Switching the toggle mid-session needs no reset:
  `apply_runtime_settings` already calls `reveal_active_tab`, both
  modes re-clamp their windows virtually, and per-mode fold/collapse
  states survive a round trip.
- The grouped renderer's UI tests opt in via `enable_agent_sidebar`;
  tests that forget it exercise the classic path by default, which is
  the shipping default.

## Validation

- Defaults, boolean spellings and reset coverage: `nebula-settings`
  tests plus the config/settings-pane getter and switch tests.
- `workspace`/`tab_scroll` unit suites cover both `resting_activity`
  semantics, both windowing maths and `agent_scoped_activity` (plain
  states hidden, `Failed` kept, agent passthrough).
- `ai_agent_sidebar_toggle_switches_renderers` draws classic rows (the
  fold hides them after the freeze clears), then flips the global
  setting and asserts group headers plus the section hairline appear
  in the same window.
- `python3 scripts/check_architecture.py --base <base>` passes; i18n
  contract tests pass with the new en-US/zh-CN keys.

## Supersedes

None.

## Revisit when

- The toggle is judged stable enough to default on: retire the classic
  path, the dispatch and the mode checks.
- Upstream ships its own grouped sidebar: evaluate converging the tree
  renderer onto it instead of maintaining a parallel one.
- Upstream renames or restructures `render_sidebar` or the two internal
  call sites: re-place the dispatch delta (a few lines).
