# A shared surface for tab menus

## Status

Implemented for the tab action and sidebar grouping menus.

## Context

The two menus need the same padding, separator, typography, and shadow, while
tab actions retain their shortcuts and grouping retains its checked state.

## Evidence

The component popup fixes the panel styling and the separator reaches the panel
edge. The tab action menu already has a single host at the workspace root, which
avoids stacking a copy of the shadow for every visible tab. A tab tooltip could
still cover the first command when the menu opened from a hovered row.

## Decision

[TabMenuSurface](../../../../../nebula_app/src/gpui_shell/workspace/tab_menu_surface.rs)
renders the flat menu items and owns keyboard selection for these two menus.
The workspace continues to anchor and dismiss tab action menus. The grouping
popover owns its trigger and return focus; it holds one menu for each opening.
Tab rows suspend their tooltips while a context menu is open.

## Rejected alternatives

- Change the global component theme to fix two menus: this would also restyle
  unrelated pickers and nested menus.
- Wrap the existing popup in another styled panel: the inner border and shadow
  would remain and the separators would still use the component's padding.

## Consequences

The commands and their workspace targets stay in the existing menu builders.
Pane menus continue to use the component popup.

## Validation

The Linux product was exercised with mouse hover, grouping selection by keyboard,
Escape, and the tab duplication action. Visual acceptance checks the visible
selection marker, shortcuts, and panel edges in light and dark themes.

## Supersedes

None.

## Revisit when

The component exposes per-instance panel and item styling that can represent
these menus with the same focus and dismissal behavior.
