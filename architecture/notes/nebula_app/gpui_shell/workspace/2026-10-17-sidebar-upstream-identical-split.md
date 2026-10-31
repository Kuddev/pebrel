# Sidebar file split keeps upstream's sidebar.rs mergeable

## Status

Decided 2026-10-17 while porting `feat/sidebar_tree` onto upstream v2.2.2.
Partially supersedes the "Legacy isolation" section of
[`2026-10-14-ai-agent-sidebar-toggle.md`](2026-10-14-ai-agent-sidebar-toggle.md):
the `sidebar_legacy.rs` verbatim-copy mechanism is gone; the toggle, dual
windowing and agent-scoping decisions of that note stand unchanged.

## Context

The 2.2.1 merge rewrote `sidebar.rs` in place (grouped renderer replacing
main's flat renderer) and parked main's flat renderer in `sidebar_legacy.rs`
(~750 lines copied verbatim from 2.2.0). Upstream keeps evolving the flat
renderer — 2.2.2 alone changed the color-strip anchor, logo slot layout,
title weight and split chrome/metadata fonts — so every upstream merge
required re-porting those fixes into the copy by hand (handover doc risk
#1), and the file itself was a ~660-line textual conflict surface that had
to be resolved merge after merge.

## Evidence

- During the v2.2.2 squash-merge, 4 of 7 conflict hunks sat in
  `sidebar.rs`'s grouped-renderer region; a further 4 upstream fixes had to
  be hand-ported into `sidebar_legacy.rs` the same day.
- `render_sidebar_slot` / `render_sidebar_title_bar` /
  `render_collapsed_tab_title` were already byte-identical to upstream in
  the merged tree — only the renderer body and small helpers were branch
  territory, so extraction was mechanical.
- The grouped renderer consumes only `pub(super)` helpers from
  `sidebar.rs` (`shell_status_*`, `tab_status_slot`, `spinner`,
  `waiting_hand`, `sidebar_new_tab_control`) plus one private const
  (`TABS_DISCLOSURE_SLOT_W`), so it compiles against an untouched
  upstream file with a tree-local const copy.
- Upstream's flat `render_sidebar` is private to the sidebar module, so
  the mode dispatch must live inside `sidebar.rs` (an appended impl block)
  rather than in a sibling module.

## Decision

- `sidebar.rs` is upstream/main verbatim except a minimal, stable delta:
  a `..` in the `TabPresentation` destructure (the branch adds
  `headline`/`is_agent_session` fields the classic path does not consume),
  two internal call sites inside `render_sidebar_slot` rerouted to
  `render_sidebar_dispatch`, and that appended dispatch impl (~24 added
  lines, all but the two call lines pure additions).
- All branch sidebar rendering lives in `sidebar_tree.rs`: the row-tree
  renderer (`render_sidebar_tree`), section/host/group headers, status
  labels, `agent_scoped_activity`, and the Ready-state resting semantics
  as `tree_resting_activity`.
- `sidebar_legacy.rs` is deleted; the OFF path calls upstream's
  `render_sidebar` directly, so classic behavior tracks upstream with zero
  porting work. `legacy_*` names revert to upstream's
  (`toggle_tabs_section`, two-arg `wrap_tabs_scroll_list`; the branch's
  four-arg variant is renamed `wrap_rows_scroll_list`), and the packed
  `TabsFold` struct dissolves back into upstream's three flat workspace
  fields (`tabs_fold_armed/frozen/seq`).
- `tab_presentation` picks resting semantics by toggle:
  `sidebar_tree::tree_resting_activity` (on) vs `sidebar::resting_activity`
  (off, upstream-identical).
- `TABS_DISCLOSURE_SLOT_W` exists twice (upstream private const + tree
  `pub(super)` const consumed by `recent_sessions`); the values must
  change together. Duplicating it is cheaper than widening the upstream
  const's visibility.
- The toggle UI test drives the fold flip by writing the workspace fields
  directly because upstream's `toggle_tabs_section` is module-private;
  the comment in `settings_navigation/ui_tests.rs` records why.

## Rejected alternatives

- Dispatching at the `render.rs` call site to keep `sidebar.rs` 100 %
  byte-identical: `render_sidebar_slot` and `render_sidebar_title_bar`
  call the renderer internally, so the tree mode would lose the fold slot
  and title-bar chrome unless those call sites route through the dispatch.
- Keeping `sidebar_legacy.rs` and re-porting upstream fixes each merge:
  exactly the drift risk this split removes.
- Widening upstream's private const or renaming its private renderer to
  `pub(super)`: each widened item is a delta that upstream edits can
  conflict with; duplicating one const is the smallest footprint.

## Consequences

- Upstream merges no longer touch branch code in `sidebar.rs`; conflicts
  there shrink to the two dispatch call lines (and the `..` destructure if
  upstream restructures that pattern).
- Remaining known merge-friction files after this split:
  `tab_scroll.rs` (dual windowing maths), `keyboard_bindings.rs` (Unbind
  model vs upstream's meta-source model) and `remote_files.rs` (follow
  model). The same split treatment is the template if upstream churn in
  them keeps forcing manual reconciliation.
- Classic-mode behavior is upstream's, including future changes we do not
  review at merge time; that is the point of the switch, but reviewer
  attention shifts to the tree path only.

## Validation

- `cargo check -p nebula --all-targets` clean; full `cargo test` in
  `nebula_app` green after the split, including both resting-semantics
  suites (upstream's event semantics in `sidebar.rs`, tree's Ready
  semantics in `sidebar_tree.rs`), `agent_scoped_activity` tests and the
  adapted toggle UI test.
- `python3 scripts/check_architecture.py --base upstream/main` exit 0.

## Supersedes

Partial — the "Legacy isolation" bullet of
[`2026-10-14-ai-agent-sidebar-toggle.md`](2026-10-14-ai-agent-sidebar-toggle.md)
only.

## Revisit when

- Upstream renames or restructures `render_sidebar` / the two internal
  call sites: re-place the dispatch delta (a few lines).
- Upstream ships its own grouped sidebar: evaluate converging the tree
  renderer onto it instead of maintaining a parallel one.
- The toggle defaults on: the classic path, dispatch and tree-side mode
  checks retire (see the 2026-10-14 note's revisit clause).
