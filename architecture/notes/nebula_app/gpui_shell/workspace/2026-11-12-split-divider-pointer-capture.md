# Split divider capture and preview

## Status

Implemented in the GPUI workspace.

## Context

A short pointer movement could leave the highlighted divider stationary until
mouse-up. A divider also needs to respond when grabbed beside its visible line.

## Evidence

The preview ratio was stored before an eight millisecond notification gate. A
move inside that interval could become the final move, leaving no repaint
request. The hover-gated overlay was also mounted after the initial press, and
a move without a reported pressed button was treated as a release.

## Decision

[The workspace capture](../../../../../nebula_app/src/gpui_shell/workspace/split_drag.rs)
listens for pointer movement and release before the press. Each changed preview
requests a GPUI frame. The visible line uses the preview while the split tree
retains its committed ratio. The hit target is drawn above both pane surfaces.

## Rejected alternatives

- Resize the pane grid on every move: that would reflow terminal content and
  enqueue PTY resizes while the pointer is still moving.
- Retain a time gate on notifications: a final move needs its own repaint.

## Consequences

Release commits once. Losing window focus cancels the preview. Grabbing beside
the divider preserves the initial distance between the pointer and the line.

## Validation

The production layout test presses on the far half of the hit target, sends two
quick moves, and checks the final line before releasing. The committed ratio
changes only on release.

## Supersedes

None.

## Revisit when

GPUI exposes a shared pointer-capture primitive for divider and sidebar resize.
