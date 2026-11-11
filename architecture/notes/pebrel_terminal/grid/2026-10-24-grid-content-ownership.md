# Grid content owns transient resources

## Status

Implemented as the terminal-core part of the OSC image repair. Protocol replay
and frontend event migration are separate dependent changes.

## Context

Absolute image anchors cannot follow partial erasure or reflow. Native result
markers already live in cells, but initialized rows outside the logical grid can
retain their strong owners until their allocation is reused.

## Evidence

The direct insertion tests exercise image fragments without OSC image handling.
A real command-capture regression uses one history row and checks the weak owner
immediately after the last logical marker disappears. The same fixture fails on
main and when only marker discard is removed from the integrated implementation.

## Decision

Cells own image identities and native markers. Placements retain weak identities;
snapshots retain only fragment coordinates. Departing cached rows release
transient owners while retaining their text allocation and actual text attributes.

Keep raw grid mutation, internal marker mutation and image insertion together in
`term/grid_content.rs`: these are the terminal's content-ownership entrypoints.
Public mutable access is conservative. Private empty grids skip cleanup and image
lookup until the corresponding content is introduced. Marker-only writes enable
cleanup without selecting image-aware snapshots. Clone, serde and cursor-template
transfer preserve the conservative contract.

Direct insertion starts a new text-input boundary and uses the existing pending
wrap operation before writing cells. It must not overwrite a completed text line.
Protocol-specific sizing and rejection still belong to the later OSC adapter.

Text-only native replacement cannot express pixels. Reject image-bearing command
sources and invalidate a previously captured source if an image is inserted later,
using the existing fingerprint pass rather than a second row traversal.

## Rejected alternatives

- Clear images only in the painter: erasure, reflow and late decode would retain
  a second model of terminal content.
- Keep owners in cached rows: logical eviction would not release their payloads.
- Track only OSC insertion: public cell mutation can introduce images too.
- Treat markers as images: ordinary native text commands lose the no-image path.

## Consequences

No image pixels are decoded here and the existing OSC Event remains unchanged.
The direct insertion API and its snapshots are complete core capabilities; the
dependent protocol change connects them to actual image output. No persistent
format or thread model changes. Hints are monotonic, not exact resource counts.

## Validation

Direct API tests cover fragment erasure, alternate screens, history eviction,
history clearing, reflow, reset, public mutation, clone, serde and cursor-template
transfer. Reflow compares source tiles over the full logical grid with enough
history to avoid intentional eviction. Native capture tests cover shared metadata, source invalidation and
marker-only cleanup. Core tests and the unchanged GPUI product configuration are
validated independently of the later protocol and math changes.

## Supersedes

None.

## Revisit when

Grid mutation gains an API that can maintain exact resource membership, or native
replacement surfaces acquire explicit image semantics.
