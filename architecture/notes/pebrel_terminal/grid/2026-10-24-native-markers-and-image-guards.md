# Native result markers and image cells share grid lifetime

## Status

Implemented while integrating the native-result and program-status changes into
the OSC 1337 image repair. No persistent format or thread model changes.

## Context

Native command capture attaches text-source markers through `Term::grid_mut`.
The image repair conservatively treats arbitrary raw grid mutation as possible
image insertion. Combining those paths would permanently select the image-aware
snapshot loop after an ordinary text command. Native markers also live in the
same optional cell metadata that image tiles preserve across grid edits.

## Evidence

- `inline_content/command.rs` captures successful, complete command ranges and
  writes first-column markers; it cannot represent OSC 1337 pixels in its text.
- `grid/storage.rs` retains initialized rows outside the logical grid. Clearing
  transient owners only when that allocation is eventually reused is too late.
- `event_loop::tests::ordinary_command_view_preserves_grid_cursor_and_chunk_boundaries`
  exercises actual command reports, marker placement, scrolling and reset.
- `inline_content::tests::image_and_marker_share_attributes_and_release_cached_owners`
  covers both insertion orders, shared metadata, attribute clearing and serde.

## Decision

Keep cached-row cleanup and possible-image lookup as separate conservative hints
on storage. New private terminal grids start with both disabled. Image insertion,
public raw mutation, public construction, cloning and deserialization remain
conservative. Internal marker-only access enables cleanup without selecting the
image snapshot specialization. Screen-template transfer preserves this distinction.

Keep the existing single optional `CellExtra` owner. Its text attributes include
the native marker; image tiles can share those attributes. Empty-text pruning must
count the marker, and cached-row discard releases both transient identities while
preserving actual text attributes. Neither marker nor image identity is serialized.

Reuse upstream command capture and source validation. A successful range containing
an image stays in the VT grid rather than becoming a text-only replacement. An
image later written into a captured row invalidates that source. The explicit
native-result protocol and ordinary text-only command capture otherwise retain
their upstream behavior.

Share the existing source-fingerprint pass for both capture eligibility and later
validation. A row containing image cells has no text-only fingerprint; it does not
need a separate full-row image scan before the same source is hashed.

Preserve both program-status protection around VTE replay and stream-owned pending
image payloads. One does not replace the other's lifetime or parser-byte accounting.

## Rejected alternatives

- Stop tracking public grid mutation: externally inserted image cells would leak
  cached ownership and disappear from the no-image snapshot specialization.
- Treat every native marker as an image: ordinary commands lose the specialization
  despite introducing no pixels.
- Let text-only replacement cards hide mixed image output: their payload has no
  image fragments, so the image would disappear with the replaced source rows.
- Create separate metadata allocations on every ordinary cell: shared rare
  attributes and the existing 24-byte cell representation remain sufficient.

## Consequences

Both hints describe what may have appeared; reset does not claim to recover exact
membership. Marker-only grids still pay cached-owner cleanup when needed, so pure
feed measurements alone do not establish final product cost. The cost fixture must
first create a real native command result and measure subsequent output as well.

## Validation

Core all-target tests cover native capture across every byte split, mixed output,
source invalidation, row filtering and image/status reset order. Reverting marker
access to public `grid_mut`, removing mixed-output rejection and removing source
image validation produces the expected regression failures.

The paired Windows release probes use the same workload and locked dependency
versions on main and the integrated core. Timing and allocator instrumentation run
in separate executables. Allocation figures are requested live bytes, not RSS or
GPU memory; shared marker/image owners are checked through discard and drop.

## Supersedes

Extends the image-only guard interpretation in
`../term/2026-09-28-no-image-grid-fast-path.md`. Its public-mutation and ordinary
no-image requirements remain in force; historical timings retain their original
revision and workload boundaries.

## Revisit when

Native result payloads gain explicit image semantics, or measured marker-only
cleanup costs justify a more precise ownership index. Do not infer either need
from the number of metadata types alone.
