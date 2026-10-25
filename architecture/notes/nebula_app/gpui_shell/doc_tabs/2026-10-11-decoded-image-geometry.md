# Image geometry follows the frame decoded by GPUI

## Status

Proposed. Platform validation results are recorded in the PR.

## Context

GPUI image tabs decode a frame in the background and use shared ImageView
geometry for pointer zoom and pan.

## Evidence

The macOS legacy image_dimensions adapter returns an unsupported error. GPUI
still displays the independently decoded PNG, but the geometry keeps dimensions
None. zoom_by then returns false and drag pan clamps to zero.

## Decision

The existing BGRA decode completion supplies RenderImage::size(0) to shared
ImageView geometry. The setter accepts only nonzero dimensions and preserves
zoom/pan. The frame used for painting is also the metadata authority.

## Rejected alternatives

- Another synchronous metadata read adds UI I/O and can disagree with the frame.
- Injecting test-only dimensions hides the actual macOS failure.
- Reimplementing anchor/clamp mathematics in GPUI duplicates the geometry rules.

## Consequences

No extra read, decode or platform dependency is introduced. Image lifecycle and
pointer geometry retain their current owner, including during live transfer.

## Validation

An isolated real PNG goes through the actual background decoder. TestWindow
wheel and drag events must change zoom and pan without injected dimensions.
Native display/DPI and platform coverage limits remain explicit in the PR.

## Supersedes

The assumption that the legacy metadata adapter always supplies GPUI geometry.

## Revisit when

A tab paints a different frame, orientation policy or animated image sequence.
