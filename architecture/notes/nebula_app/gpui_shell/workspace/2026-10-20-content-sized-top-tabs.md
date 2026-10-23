# Content-sized top tabs

## Status
Implemented.

Overflow behavior superseded by [interactive tab titles](2026-10-20-interactive-tab-titles.md).

## Context
Equal-width tabs consumed a full 220px for short names. Three horizontal tab
treatments also needed to share activation, closing and ordering behavior.

## Evidence
[The top tab renderer](../../../../../nebula_app/src/gpui_shell/workspace/top_tabs.rs) previously divided the viewport by the tab count and
used that same fixed pitch for dragging. Applying variable widths only in CSS
would leave overflow buttons and drag destinations using the wrong positions.
Occluding tab hitboxes also suppress ancestor hover listeners. Top tab rows
therefore receive movement directly; an unhovered-root release handler covers
fast drops before the full-window drag overlay has painted.

## Decision
Measure names with GPUI's text shaper and reserve the existing identity, status,
administrator and split-count slots. Cap each tab at 220 logical pixels and use
an ellipsis plus the full-title tooltip. `top_tabs/layout.rs` owns the prefix
positions used by layout, arrow navigation and drag ordering. Each horizontal
gesture freezes that layout and captures the starting scroll offset; auto-scroll
is included in pointer displacement. The pinned settings tab stays outside the
reorderable range. Sidebar gestures retain their row pitch.

`nebula_settings::TabStyle` owns the saved `tab_style` values: `chrome` (default),
`minimal`, and `tag`. Appearance settings persist through the existing settings
boundary; open windows use the cached global snapshot. Styling has no tab state.

## Rejected alternatives
Intrinsic widths without measured positions would leave ordering and overflow
navigation inconsistent. Text marquees add perpetual movement; ellipses meet the
requested overflow behavior while preserving a stable titlebar.

## Consequences
Long titles no longer force short titles to stretch. The existing left sidebar
layout and session metadata keep their semantics.

## Validation
[Core preference round trips/reset](../../../../../nebula_settings/src/tab_style.rs); mixed-width navigation and ordering rules;
rendered tests for all three styles, CJK/long titles, hover close and bidirectional
dragging; [existing settings navigation and density coverage](../../../../../nebula_app/src/gpui_shell/workspace/settings_navigation/ui_tests.rs).

## Supersedes
None.

## Revisit when
The maximum tab width becomes user-configurable or window chrome changes its
reserved toolbar geometry.
