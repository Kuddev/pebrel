# Keep formula geometry stable during ordinary terminal selection

## Status

Implemented for review of issue #119's GPUI drag-selection behavior.

## Context

Inline formula projection compresses source columns into a smaller visual box.
The previous GPUI gate cleared the projection as soon as a selection acquired a
nonempty range. Text then moved beneath the pointer midway through the gesture,
and later pointer events used different coordinates from the initial press.

## Evidence

The previous `effective_selection_clears_existing_projection` regression encoded
that behavior. `TerminalView::grid_point` already uses the shared
`TerminalMathState::source_point` mapping, which snaps a formula's visual halves
to its source boundaries. Terminal selection and clipboard serialization retain
the original grid text independently of the rendered formula.

## Decision

Keep formula planning and projection for simple and whole-line selections. The
selection range is captured with the same terminal snapshot and carried through
frame finalization. Intersecting formulas receive an outline using the existing
selection color. Their bitmap foreground and raster key remain unchanged.

Vi, block and semantic selection retain their source view. Rectangular selection
needs a different mapping when formulas shift individual rows by different amounts;
semantic expansion also operates on source words. This change does not claim
those interactions have acquired projected selection semantics.

## Rejected alternatives

- Clear projection at the first nonempty selection: moves content during dragging.
- Keep the image but clear hit mapping: the pointer selects different source text.
- Rewrite the terminal grid into rendered text: changes copying and terminal state.
- Recolor every selected formula bitmap: adds selection-dependent raster variants
  and can temporarily blank a formula while a new image becomes available.

## Consequences

Selection frames run normal formula planning instead of the old source-only fast
path. Geometry, layout caching and resource budgets remain in their existing owners.
The source/raster key does not change with selection; regression assertions compare
source, foreground and fitted size. Selection intersection uses the core range
predicate over covered source spans, with an early row rejection and no extra
per-cell collection. Painting adds one outline per selected visible formula.
These are bounded cost properties, not a frame-rate claim.

The terminal's source selection remains authoritative for copying. The GPUI
presentation policy changes without adding a second parser or coordinate mapping.

## Validation

Regressions cover stable suffix projection, atomic source copying, selections on
another row, source-column selection modes, the two reported formula bodies, and
a real GPUI mouse drag with clipboard verification. Native visual/performance
acceptance remains distinct from these behavior tests.

## Supersedes

None.

## Revisit when

Block/semantic selection gains a projected-coordinate model, or profiling of a
large formula viewport identifies a concrete selection-planning cost problem.
