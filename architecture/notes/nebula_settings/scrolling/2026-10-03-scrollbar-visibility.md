# Terminal scrollbar visibility

## Status
Proposed implementation for Issue #425.

## Context
The overlay scrollbar disappears at the live bottom even when scrollback exists.
Mouse users cannot grab it without first scrolling, and dragging to live loses feedback.

## Evidence
`terminal/view/pointer.rs` previously returned no thumb for offset zero. The
24px minimum thumb also made the old inverse mapping diverge from painted geometry.

## Decision
Shared settings parse and reset `scrollbar_visibility=auto|hover|always`, default
Auto. Auto displays in history, Hover displays in the existing right-edge hit band,
and Always displays whenever history exists. Active drags stay visible in every
mode; no history always means no scrollbar or input interception. Hot application
updates existing terminals. Hover clears when leaving the terminal.

Rendering and input share the same visible thumb and right-edge bounds. The drag
mapping uses the actual thumb travel range so minimum-size thumbs reach both ends
without jumping when first grabbed at live bottom.

## Rejected alternatives
A boolean omits the requested hover mode. Separate visible/hit geometry can make
hidden controls consume selection or TUI input. Drawing an empty disabled bar adds
no usable scrollback and occupies the terminal edge without purpose.

## Consequences
No grid width is reserved and no shell/PTY behavior changes. Hover is owned by the
view; the shared model contains only stable preference values.

## Validation
Regression coverage checks persistence/reset, real dropdown choices, hover/leave,
selection when hidden, live dragging with a minimum-size thumb, and outside release.
Execution and visual/platform acceptance are reported separately through Actions.

## Supersedes
None.

## Revisit when
Scrollbar geometry or the terminal input ownership model changes.
