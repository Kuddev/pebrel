# Inline images share the finalized text projection

## Status

Proposed; verified with Windows GPUI bitmap/layout tests.

## Context

OSC 1337 now leaves the cursor after the last image row. A main-screen line
can therefore contain a compact formula, an image and a text suffix. Text
used the finalized formula projection while both image painters used raw
columns. A valid `$x^2$ ` + four-cell image + `TAIL` sequence placed the image
at pixels 48..80 and T/A at 64..80 with eight-pixel cells.

## Evidence

The real PNG decoder and asynchronous formula bitmap pipeline reproduced the
overlap. Alternate-screen, active-input-line and ANSI-background controls did
not compact the line. A separate regression demonstrated that formula
recognition treated image placeholder spaces as TeX source.

## Decision

`LineProjection` projects image fragments for both renderers. Fragments retain
texture source row/column and merge only when visual and source coordinates
are both contiguous. Existing full-image sampling and destination clipping
remain unchanged. Each row follows the same projection as its text and hit
testing; a multi-row image may therefore have differently shifted slices.

Formula candidates cannot consume occupied image cells. This check intersects
the candidate with the current visible scan rectangle: persisted display
formulas can legitimately extend beyond a viewport shortened by a resize.
The height-shrink regression demonstrated an out-of-grid access in an initial
implementation that checked the untrimmed persistent spans.

## Rejected alternatives

- Disable every formula on an image-bearing row: unrelated formulas can still
  render and the common row mapping already defines their geometry.
- Shift a whole image by its first row: other rows have independent projections.
- Project only the first cell of each run: GPUI captures image runs and scans
  formulas under separate terminal locks. A formula written between those
  captures can split an older run; mapping every cell preserves its surviving
  fragments and texture offsets.
- Stretch each surviving fragment: erasure and clipping must not change UVs.
- Move source cells or copy a projection algorithm into the painter: selection,
  protocol cursor positions and formula hit testing must retain one authority.

## Consequences

Only present image runs require fragment work; empty run lists allocate nothing.
Formula/image occupancy checks run on visible candidate spans. Core grid and
Cell/Row layouts are unchanged by this projection repair. The existing GPUI
snapshot/math-plan lock separation is not changed by this patch.

## Validation

The production PNG and asynchronous math-bitmap regression passes with the
image at 32..64 and TAIL at 64..96 in the reproducer. Tests cover identity
controls, erased tiles, texture row offsets, right clipping, expanded formulas,
inverse pointer coordinates, occupied formula spans and viewport height shrink.
A deterministic two-capture regression also overwrites the middle of an image
with a formula and verifies the fragments projected from the older snapshot.
These automated checks verify geometry and resources, not a new final-GPU
screenshot or a claim about which combinations OMP itself emits.

## Supersedes

None; refines the grid-owned image rendering contract.

## Revisit when

Image blocks acquire a different layout model or terminal snapshots carry the
complete finalized presentation plan.
