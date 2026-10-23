# Buffered local SFTP file I/O

## Status

Proposed for review.

## Context

Desktop transfers already pipeline remote requests. The transfer engine nevertheless
reads or writes the local file once per 32 KiB protocol chunk. Tokio regular-file
operations use blocking workers, so the small local operations add scheduling and
copy costs even when the network has spare capacity.

## Evidence

An isolated ARM64 Linux probe used the production transfer functions, an in-memory
SFTP peer, real local files, two runtime workers and 64 download requests. Three
runs transferred 64 MiB plus 113 bytes and compared all destination bytes.
Baseline upload rates were 412.3, 387.1 and 399.4 MiB/s; buffering yielded 537.5,
618.3 and 582.0 MiB/s. Download rates were 622.7, 579.5 and 604.8 MiB/s before,
and 675.3, 708.9 and 675.8 MiB/s after. These measurements isolate local overhead;
they are not Windows or WAN throughput guarantees.

## Decision

Keep packet size and network concurrency in `limits.rs`. Buffer local uploads
with a 256 KiB reader and batch local download writes up to 256 KiB per range.
Small ranges allocate only their length. The default 64-worker download retains
at most 16 MiB of batch payload, in addition to existing protocol/runtime buffers.
Cancellation remains checked at every remote chunk. Download progress advances
only after each local batch has been written and flushed to the OS; this is not
an fsync or a durable-storage guarantee.

Treat premature remote EOF as an error. Accepting a short range can otherwise
publish a truncated file or a hole before a later concurrent range. The existing
atomic download caller removes its temporary output on failure.

## Rejected alternatives

- Larger network packets: conflate disk batching with server packet limits.
- More concurrent remote requests: do not address per-chunk local scheduling.
- Unbounded whole-file buffering: unnecessary memory pressure and worse cancellation.
- Reporting a short range as success: violates the requested file-length boundary.

## Consequences

Uploads retain an extra read-ahead buffer. Download progress is updated in batches
instead of every 32 KiB, while cancellation checks remain fine-grained. Partial
buffer contents are discarded on cancellation or read error. This change does
not add resume persistence or alter the caller's atomic-transfer policy.

## Validation

The SFTP-filtered application suite passed 49 tests on ARM64 Linux. The new real
codec fixtures cover empty, short and segmented files, repeated short reads,
unchanged packet sizes, premature EOF with windows 1/4/64, cancellation without
counting unwritten buffered bytes, and remote upload errors. Architecture,
formatting and diff checks are required before submission. Windows native CI and
real-network performance remain separate validation.

## Supersedes

None.

## Revisit when

Representative Windows/network measurements identify a different bottleneck,
the file runtime changes, or a resumable caller needs explicit committed offsets.
