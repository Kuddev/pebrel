# Sidebar order belongs to the sidebar

## Status

Implemented for the sidebar tab menu.

## Context

The group menu offers creation order, recently used tabs, and manual order. It
also inserts a divider below the selected tab or hides the members of a group.

## Evidence

The workspace tab index addresses shortcuts, the top bar, and drag operations.
Group headings already occupy rows in the sidebar scroll window. Dividers and
collapsed groups change those row positions without changing which tabs exist.

## Decision

[SidebarTabRows](../../../../../nebula_app/src/gpui_shell/workspace/sidebar_groups.rs)
projects stored tab indices into visible rows. Creation and activation stamps
and the divider anchor follow each live tab; the window owns grouping, ordering,
and collapse state. Manual order retains the workspace's existing drag order.
A hidden member resolves to its group heading when scrolled into view.

## Rejected alternatives

- Sort the workspace vector: sidebar display preferences would change numbered
  shortcuts and the top tab bar.
- Attach dividers to visible row numbers: sorting or closing a tab would move
  the divider to an unrelated tab.

## Consequences

Sort and collapse affect the sidebar only. Session recovery keeps its existing
format. A divider is attached to the live tab and is shown in ungrouped mode.

## Validation

Projection tests cover a recently used group with hidden members, directory
identity, and the manual drag boundary.

## Supersedes

None. Extends [the original sidebar projection](2026-11-11-sidebar-tab-groups.md).

## Revisit when

Sidebar view preferences become part of session recovery.
