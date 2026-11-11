# Image fragments use the native result viewport

## Status

Implemented with the upstream native-result projection and shared math scanner.
Runtime evidence is limited to the tested native configuration.

## Context

Native results replace source-row extents with natural-height cards. Terminal
images now follow surviving grid fragments rather than an absolute whole-image
anchor. Keeping the old image painter's row-times-height calculation would put
text and images in different vertical coordinate systems after a result card.

Math scanning also needs the source rows behind a compressed viewport. Filtering
hidden formulas only after scanning lets them consume the existing 64-formula
budget before a visible tail formula is considered. Image placeholder spaces are
not empty layout room for adjacent tall formulas.

## Evidence

- `native_results/viewport.rs::Projection` already owns source/visual row mapping,
  replaced-row coverage and the number of source rows needed by the viewport.
- `inline_image.rs::image_geometry` keeps full texture bounds separate from the
  surviving cell clip; changing visual position must not change texture origin.
- `math_overlay` regressions cover a cached set of 64 hidden formulas followed
  by a visible formula, and tall formulas between image rows.

## Decision

Reuse `Projection::row`, `source_rows` and `covers`. Capture the upstream-selected
rows, apply the existing formula column projection to image runs, then map their
vertical origin through the same function as text. Source row/column coordinates
still describe the original image and determine its full texture bounds.

Pass row visibility into the shared math scanner before its scan/cache limits.
Its internal text-grid representation marks hidden rows and image cells as
non-text objects, rejects formula extraction across them and treats them as solid
layout boundaries. This representation also invalidates cached formulas when the
visibility or image occupancy changes. The existing formula limit is unchanged.

An attachment row keeps its height even when a formula on an adjacent row shifts
horizontally. Borrowing height in unprojected columns cannot establish safety
after those rows receive different column projections. The padded-prefix regression
reproduces this overlap and keeps the final clip within the formula's own row.

Keep one scanner, formula fit calculation and image geometry implementation.
Legacy rendering includes all its rows but shares the image-occupancy rule.
Its image adapter owns capture, projection and batch drawing; the display
entrypoint only schedules that batch between cells and window chrome.

## Rejected alternatives

- Keep a second image-specific vertical map: it would duplicate native result
  height, scrolling and viewport-origin ownership.
- Modify image source coordinates to move it: that changes texture sampling and
  stretches or clips erased fragments incorrectly.
- Raise the formula limit or filter only the final list: neither prevents hidden
  source from consuming the bounded scan before visible output is considered.
- Treat image spaces as prose: even the prose antialiasing allowance can overlap
  pixels; occupied image rows cannot lend that sliver.

## Consequences

Natural-height result geometry remains owned by upstream. Images and formulas
consume its visibility contract without changing the VT grid or PTY dimensions.
The internal object representation is not written to the terminal or persisted.
Geometry tests do not claim a new live OMP session or screenshot acceptance.

## Validation

Production image geometry is tested with compressed and expanded native cards,
multiple fragments, scrolling, source offsets and an unchanged full texture size.
Math tests preserve a visible tail across two filtered frames after first filling
the unfiltered cache, and bound a tall formula's clip between real image cells.
Existing real PNG decode, math bitmap and partial-erasure regressions remain.

## Supersedes

Extends `2026-10-13-images-and-math-projection.md` with the native-result vertical
projection. Horizontal formula projection and bitmap-readiness rules remain.

## Revisit when

Native results support image-bearing replacement surfaces, or another renderer
requires a distinct projection contract demonstrated by a failing behavior case.
