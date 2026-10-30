# Ordered graphics placements and shared image resources

## Status

Implemented for the direct, uncompressed RGB/RGBA profile. Review and validation
are limited to that profile rather than claiming the entire graphics protocol.

## Context

The first implementation mutated the grid while VTE could still be buffering a
DEC 2026 frame. It conflated a bitmap with one screen-space bounding box, lacked
transfer/placement replies, and marked publication complete before a saturated
frontend queue could accept it.

## Evidence

The [protocol review references](https://github.com/Kuddev/pebrel/pull/767) defines
independent image/placement identities, source crops, first-chunk metadata,
quiet-response modes, and cursor movement by the complete placement row count.
The upstream `graphics.c` linked in the protocol review deletion filters distinguish visible placements
from an explicit image/placement ID. VTE 0.15.0 has no general APC handler hook.

## Decision

Reuse the existing private, one-shot OSC title markers for ordered operations.
The input stream holds command payloads and the protocol state. The Term holds
only weak pending references; dropping a disconnected stream releases unfinished
uploads even if the failed pane retains its Term. An uncontended, stream-local
mutex permits the VTE callback to access that same state without a second owner.
Lock order is always Term before graphics state. Capability queries are pure
bounded validations and can answer without committing a synchronized frame.

Pending markers use keyed lookup and FIFO release rather than rescanning a growing
vector for every chunk. The existing 16-item/32-MiB OSC encoded-data admission
remains. Graphics command metadata has a 65,536-marker ceiling within the same
32-MiB pending-byte budget; VTE's existing bounded synchronization buffer normally
commits much earlier. Rejected uploads receive the protocol budget error.

Image IDs are scoped to the primary or alternate screen. Named images retain one
raw allocation. Each placement has its own grid-owned lifetime, crop and scale;
render snapshots keep weak placement lifetime and immutable geometry. All its
placements select the same image resource/texture. Replacing one placement does
not cancel another. Bitmap retransmission/quota eviction invalidates its resource
lease, including stale snapshots and in-flight decode, without walking scrollback.

The raw store keeps 16 images and charges their eventual BGRA cost against 128 MiB.
Transfers keep one bounded base64 buffer. Queries never replace named data.
The final chunk uses the first header's action/IDs and the applicable quiet mode.
The supported selectors are a/A and i/I, including explicit placement IDs.

graphics decode jobs hold weak raw-data references and have 16 metadata slots separate
from the existing OSC encoded queue. The core already owns/bounds those raw bytes,
so they do not consume the OSC 32-MiB encoded-data budget. Queued, active and cached
resource IDs suppress duplicate decodes; a later placement can retry a texture
evicted by the unchanged 16-image/128-MiB frontend cache.

## Rejected alternatives

- End synchronization for every APC chunk: breaks atomic frames and causes excess
  work. Only real replay points perform grid changes.
- Move the raw store into the retained Term: extends upload lifetime on disconnect.
- One union rectangle for all placements: loses independent crops and positioning.
- One decoded bitmap per placement: multiplies memory for tiled clients.
- Treat unsupported file, PNG, Unicode-placeholder or relative-placement controls
  as ordinary placements: returns success for a different operation.
- Increase the encoded queue's byte cap: leaves duplicate ownership and starvation.

## Consequences

The initial profile supports direct RGB/RGBA, chunking, query/quiet replies,
anonymous display, source crop, c/r sizing, pixel offsets, C=1, independent
placements and scoped deletion. PNG/file/shared-memory/compression, Unicode
placeholders, animation, relative placements and nonzero z-order return ENOTSUP.
This remains a grid-backed profile, not an arbitrary floating overlay renderer.

Per-stage bounds are not a process/GPU working-set promise: encoded input, one
bounded decode temporary, BGRA cache, allocator and graphics-driver costs remain
distinct. No dependency or thread pool is added. Existing OSC 1337 stays on its
original transport and shared rendering path.

## Validation

Small wire fixtures cover synchronized cursor/reset order, 400 small chunks,
queries during synchronization, response metadata and quiet modes, main/alternate
screen identity, selective deletion and default cursor movement. Existing OSC and
grid image suites protect ordering, reflow, attributes and lifetime.
Frontend cases cover shared weak queue admission, decode deduplication/expiry and
crop geometry at a viewport edge. Real-client screenshots are separate evidence.

## Supersedes

Extends `2026-10-30-bounded-transfer-store.md`: the store now charges BGRA cost,
placements are independent, and ordered replay/response/admission contracts are
specified together. It does not retroactively broaden the earlier test evidence.

## Revisit when

A client needs an unsupported transport or overlay feature, or measured workloads
justify a different resource budget. Add that capability as a complete verified
path, preserving stream ownership and one decoded resource per image.
