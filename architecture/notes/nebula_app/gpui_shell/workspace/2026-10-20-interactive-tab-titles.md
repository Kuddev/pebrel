# Interactive long tab titles

## Status
Implemented.

Superseded by [static tab overflow](2026-10-20-static-tab-overflow.md).

## Context
Long names must be readable while selected or hovered, while inactive tabs retain
an ellipsis. A scrolling title must not move the identity, shell prefix or close button.

## Evidence
[The top tab renderer](../../../../../nebula_app/src/gpui_shell/workspace/top_tabs.rs)
already measures titles and reserves fixed control slots. GPUI's keyed element
state owns hover only while the element is rendered; its animation wrapper owns
the clock and honors the application reduced-motion preference.

## Decision
Retain one hover flag per rendered tab using GPUI keyed state. Only overflowing,
visible, selected/hovered titles in the active window receive a repeating
animation. [The label](../../../../../nebula_app/src/gpui_shell/workspace/top_tabs/label.rs)
clips its own text, pauses at either end and scrolls back at 32 logical pixels per
second. Removing the animation restores ellipsis and discards its clock. No
workspace timer or persistent settings are added. The selected terminal's muted
shell prefix stays outside the moving name and replaces the former trailing
shell indicator. The trailing close/status slot retains its size.

## Rejected alternatives
Animating entire tabs would move controls and hitboxes. Animating every long
name would schedule frames for inactive and offscreen tabs. A second task-based
timer would duplicate GPUI's element lifetime and reduced-motion handling.

## Consequences
At most the selected and hovered visible long titles move. Short titles and
inactive windows remain static. Selecting a terminal adds its shell prefix to
the measured width, still capped at 220 logical pixels.

## Validation
[Rendered interaction coverage](../../../../../nebula_app/src/gpui_shell/workspace/settings_navigation/ui_tests.rs)
checks hover over tab padding, selection after pointer exit, idle ellipsis,
short names, fixed tab width and separation from the close button.

## Supersedes
The ellipsis-only overflow choice in
[content-sized top tabs](2026-10-20-content-sized-top-tabs.md).

## Revisit when
Text needs a different scrolling speed, or tab controls change their width.
