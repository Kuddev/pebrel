# Nested JSON and native-result text selection

## Status

Implemented; native regression and desktop acceptance evidence are reported with
the change rather than inferred from compilation.

## Context

Flattening JSON objects into table cells hides nested fields and conflates null
with missing values. The requested view needs expandable values, short inline
arrays and informative collapsed previews. Markdown already enabled selectable
TextViews, but its result container stopped the bubble events used by the
component library's window selection controller.

## Evidence

The locked TextSelectionController starts and extends a selection in bubble
phase. Div mouse listeners also require their hitbox to be hovered. The native
result's occluding hitbox provides isolation from the underlying terminal; stopping
all left-button/move events prevents the separate window selection controller.
The mathematical horizontal scrollbar uses its own absolute bounds for placement,
so its bounds must explicitly match the individual formula region.

## Decision

Preserve JSON types and field order in an immutable, bounded model parsed off the
UI thread. Result entities own expansion and filtering state. Default expansion
shows the root and first-level objects; short scalar arrays stay inline. Collapsed
objects preview two fields; arrays show count and available identifier values.
Build visible rows on state changes and instantiate only viewport text views.
Retain the original source independently for exact copying and raw display.

Let left press/move events reach window text selection while retaining occlusion,
right/middle-button handling and vertical terminal scrolling. Copy selected text
when a selection exists; the explicit copy-source control still copies raw output.
Place each formula scrollbar inside a four-edge absolute wrapper belonging to
that formula, with only its local scrollbar clearance.

## Rejected alternatives

- Recursively parse preview strings from flattened cells: loses JSON type/source
  distinctions and cannot reconstruct the original hierarchy reliably.
- Mount a separately scrolling tree widget: creates another vertical viewport
  instead of using the terminal's retained-output projection.
- Turn off text selection or stop all mouse events for isolation: breaks reading.
- Expand all nested data on each render: repeats parsing/layout and loses state.

## Consequences

JSON changes from sortable records to a navigable tree; directory listings keep
their existing tables, sorting and filters. Expansion survives raw/view switches.
Parsing respects the existing retained-content and node budgets and rejects stale
results. Collapsed previews are bounded; scalar rows retain complete keys and values,
including whitespace, for horizontal reading and selection.

## Validation

The focused tests cover inline arrays, nested previews, identifier hints, filtering,
real fold controls, cross-row selected-text copying, long fields, whitespace,
raw/view retention and PTY isolation.
Directory sorting/hover regressions remain on actual directory-listing inputs.

## Supersedes

The unconditional mouse bubble guard described in
[hover-state-and-natural-height](2026-10-21-hover-state-and-natural-height.md).
Its keyed-hover and fractional-height decisions remain unchanged.

## Revisit when

The component selection contract changes or a structured protocol provides node
identity across incremental JSON updates.
