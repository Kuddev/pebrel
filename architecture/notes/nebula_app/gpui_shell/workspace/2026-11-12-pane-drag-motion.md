# Live pane movement during exchange

## Status

Implemented for the existing pane grip.

## Context

The pane exchange gesture indicated its destination with text, but the panes
did not move until the user let go.

## Evidence

The workspace already captures the grip gesture at the window and records
each leaf's committed bounds for hit testing. A pane's contents can move with
the pointer while its original slot keeps those bounds.

## Decision

Render the carried pane above its siblings at the pointer displacement.
Slide the destination toward the source slot and reverse that offset when
the pointer leaves. The unmoved slot records geometry until release commits
the exchange of leaf IDs. Reduced motion skips the destination tween.

## Rejected alternatives

- Swap leaf IDs at hover: unequal panes would reflow terminal grids during
  the gesture and hit testing would follow moving targets.
- Draw a text label alone: the panes would still look stationary.

## Consequences

The preview changes positions without changing dimensions or moving PTY
ownership. Release keeps the existing swap or detach behavior. Losing
window focus clears the preview.

## Validation

The grip interaction tests exercise movement, cancellation and exchange
with the existing pane entities. The native window shows the carried pane
and the destination's displacement while the mouse is still held.

## Supersedes

None. Extends [the pane exchange gesture](2026-10-03-pane-position-exchange.md).

## Revisit when

Pane dragging supports inserting new split positions.
