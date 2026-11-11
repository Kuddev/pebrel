# Sidebar tab grouping and row geometry

## Status

Implemented in the GPUI workspace.

## Context

The Tabs menu groups terminals by working directory or shell. A group divider occupies
a visible row, and tabs from the same group may be adjacent on screen even when their
indices in the workspace are far apart.

## Evidence

Before grouping, sidebar scrolling and dragging used tab counts and tab indices to
locate every row. Counting tabs alone would omit the final rows from scrolling and
send a drag to the wrong tab once headings occupy their own rows.

## Decision

Use the [sidebar projection](../../../../../nebula_app/src/gpui_shell/workspace/sidebar_groups.rs)
for rendering, scrolling, revealing the active tab, and mapping vertical drags back
to workspace indices. Drag targets remain within the source group. Switching group
mode leaves the workspace order and active tab intact.

The projection reads directory, host, and shell information already owned by the
workspace and terminal views. A directory key includes its full path and WSL scope.
The default ungrouped projection uses only the tab count.

## Rejected alternatives

- Reordering the workspace when the filter changes would also change keyboard
  cycling and the top tab bar.
- Looking for a repository root during render would put filesystem work on the UI thread.

## Consequences

Group headings count toward scroll extent and cannot receive drops. The grouping
mode belongs to the window and does not add a saved preference.

## Validation

Projection tests cover interleaved members, distinct directories with the same name,
WSL scope, and the drag boundary. The
[sidebar scroll tests](../../../../../nebula_app/src/gpui_shell/workspace/tab_scroll.rs)
cover the shared row window calculation.

## Supersedes

None.

## Revisit when

Tabs gain a project identity independent of the current terminal directory or
grouping is saved across sessions.
