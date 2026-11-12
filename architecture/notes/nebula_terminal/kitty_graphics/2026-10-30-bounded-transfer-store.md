# Bound graphics transfers without conflating data and placement lifetimes

## Status

Implemented in the local PR review worktree. This budget repair is not a statement
that the complete graphics PR is ready to merge.

## Context

Frontend decode/cache limits do not constrain the protocol layer's raw image map
or incomplete base64 buffers. Conversely, dropping raw data whenever the last
visible cell disappears breaks the protocol's separate transmit/place lifecycle.

## Evidence

The [protocol review references](https://github.com/Kuddev/pebrel/pull/767)
distinguishes lowercase placement deletion from uppercase data deletion and
requires deletion commands to abort incomplete uploads. A headerless command for
an already stored ID must not create another independent partial transfer.

## Decision

[`KittyGraphics`](../../../../nebula_terminal/src/kitty_graphics.rs) owns one active
partial transfer and a FIFO-bounded named data store: at most 16 completed images
and 128 MiB of raw payload bytes. Incomplete base64 is limited to 24 MiB and to
the declared pixel count's encoded size, whichever is smaller. Append limits are
checked before joining or decoding. Capacity grows geometrically within that
limit rather than reallocating for every small chunk.

Starting a new header abandons the previous partial context. A headerless new ID
does not borrow stale dimensions from its completed image. Any delete command
aborts the incomplete upload.

Grid cells own placement identities; the protocol registry and event payloads
hold weak references where they must not prolong a placement or queued pixels.
Losing the last cell drops placement metadata, but named raw data remains in the
bounded store for a later `a=p`. Uppercase data deletion and FIFO eviction release
that stored data. A retransmission establishes new dimensions and identity.

## Rejected alternatives

- Rely only on the frontend's cache limit: leaves core maps unconstrained.
- Keep a strong placement identity in the protocol map: grid erasure no longer
  invalidates frontend work.
- Delete raw data whenever a placement dies: breaks re-display without retransmit.
- Reserve exactly one additional chunk each time: can repeatedly relocate an
  increasingly large buffer.

## Consequences

These are per-stage logical payload budgets, not a 128 MiB process-memory promise.
Encoded input, base64 decode temporaries, one background decode, frontend BGRA
cache, allocator overhead and GPU storage have separate lifetimes and costs.
The existing frontend queue/cache and synchronization semantics still require
their own review; passing the core regression set does not validate those paths.

## Validation

Small fixtures cover count eviction, oversized final chunks, abandoned/headerless
uploads, lowercase re-placement, uppercase release, deletion during an incomplete
upload, chunk boundaries and declared-cell scaling. No large-allocation or
reported machine-freezing reproduction is required by these tests.

On 2026-10-30 the Windows core run passed 16 graphics tests and two cell-size query
tests. Architecture and whitespace checks passed against main `1b58f5dd`.
Full frontend rendering, synchronization and PR readiness are not established by
those checks.

## Supersedes

None.

## Revisit when

Transport, placement or decode ownership changes, or measured workloads justify a
different budget. Such changes must preserve bounded incomplete transfers and the
separate stored-data/placement lifetimes.
