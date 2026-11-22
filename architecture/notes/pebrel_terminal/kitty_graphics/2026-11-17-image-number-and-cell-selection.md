# Image numbers and current-cell selection

## Status

Implemented for review of PR #819.

## Context

The proposed control extensions interpreted `d=p` as a placement ID and `I`
as a count of newest images. They also stored a placement's initial row and
column to resolve later cell deletions. Those coordinates do not follow grid
scrolling or reflow.

## Evidence

Kovid Goyal’s terminal graphics specification (sections “Deleting images”
and “Requesting image ids from the terminal”) defines `d=p` through one-based cell coordinates `x` and `y`. `d=n` selects the
newest resource with image number `I`. Multiple resources may share a number;
their assigned image IDs remain unique. Supplying both `i` and `I` is invalid.
Feature queries return supported, non-default values for the requested action.

## Decision

The core bitmap store owns the image-number association and allocated IDs.
Numbered transmissions allocate an unused nonzero ID, retain the submitted
number, and reply with both identifiers. Placement and number-based deletion
resolve the newest retained match in the active screen's existing namespace.
Lowercase deletion retains its data; uppercase deletion may release it, making
an older retained resource with the same number addressable again.

The grid remains the authority for occupied cells. Cell, cursor, row and column
deletions select placement identities from current active-screen cells. The
protocol boundary converts one-based coordinates; there is no extra origin
copy to synchronize after scrolling. Identity/range deletion includes retained
resources with no placements, while still preserving data referenced elsewhere.

Capability replies omit default and irrelevant fields. They do not advertise
independent z ordering: accepted signed `z` metadata uses the existing overlay
paint order after glyphs. `o=z` is a separate unsupported compression request.

## Rejected alternatives

- Deleting the last `I` resources can erase unrelated images with other numbers.
- Retaining initial placement coordinates creates a second, stale geometry source.
- Enlarging cache limits does not correct identity or selection semantics.
- Echoing every recognized key advertises behavior the queried action ignores.

## Consequences

No new dependency, worker or persistent format is introduced. Resource count
and byte budgets still bound the store. A new explicit format header replaces
an incomplete PNG upload even when both transfers omit external dimensions.
The ordinary PNG query checks container geometry without retaining an image.
PNG preflight also accounts for source sample depth against the existing decoder
allocation bound, so oversized 16-bit source buffers are rejected before admission.

## Validation

Protocol regressions cover assigned-ID uniqueness, repeated image numbers,
lowercase retention, uppercase release, coordinate origin, scrolling/reflow,
unplaced resources, invalid ranges and action-specific feature replies. PNG
cases cover standard queries, replacement headers and source sample bounds.

## Supersedes

None.

## Revisit when

An independent image plane or z ordering replaces cell-projected placements.
