# Inline result hover state and natural height

## Status

Implemented; the focused regression fails before the row-identity change and
passes afterward. Physical pointer acceptance accompanies the review.

## Context

JSON row backgrounds followed the pointer only when another event redrew the
result. The following prompt could also acquire extra vertical space after a
native result, even though the underlying terminal output had not added a line.

## Evidence

The pinned GPUI Div implementation creates hover state only for elements with an
element identity. Its Capture listener notifies the current view only when that
state exists. A debug selector does not supply identity. The result's Bubble
mouse-move guard runs after this Capture listener and does not cancel its notify.

The cached-parent regression crosses real JSON row bounds and observes result
notifications before forcing another draw. It fails without row identity. Tests
that force a draw immediately after every pointer move conceal this failure.

The shared projection previously rounded measured height up to an integral
terminal row. This adds up to almost one row of presentation-only space before
subsequent terminal content.

## Decision

Key each row by its source-row index within the owning result. Sorting and
filtering preserve the row's identity while its displayed position changes.
Keep the existing event guard so native controls retain pointer ownership.

Retain fractional row height in the existing source-to-visual projection.
Rendering, source mapping, selection and following prompts continue to use one
projection. Native body padding is symmetric; original shell newlines remain
source data and are not removed to conceal projection padding.

## Rejected alternatives

- Redrawing the whole terminal for every mouse move: hides missing hover state
  and spends work even when the pointer stays within the same row.
- Removing the native result's mouse guard: can allow terminal gestures through
  the controls without establishing a row's own hover state.
- Resizing the PTY or deleting blank grid rows: changes terminal content to fix
  a presentation-space calculation.

## Consequences

Crossing row boundaries now schedules a repaint. CPU totals from the previously
under-redrawing implementation are not a responsiveness target. No parsing or
background discovery is added to hover handling.

## Validation

Thirty reading interaction tests, three projection tests and fifteen pointer
tests passed locally. They cover cached hover notification, actual table heights,
fractional inverse mapping, original directory links, selection, scrolling,
filters, sorting and structured diff controls. Native desktop comparison remains
separate from synthetic-window execution.

## Supersedes

Corrects hover identity and row quantization in the native result renderer.

## Revisit when

The GPUI dependency changes stateless hover invalidation, or the terminal adopts
a different shared coordinate authority.
