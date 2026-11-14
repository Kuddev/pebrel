# Concurrent mobile upload preparation

## Status
Proposed for review.

## Context
Android prepared each source batch only after the previous batch's write
acknowledgements arrived. Reading a document provider and encoding Base64 left
the network idle between batches. A provider returning short reads also reduced
the effective native request window to the size of one local read.

## Evidence
The previous upload loop in `SftpClient.kt` read, encoded and awaited each batch
in sequence. Native `transfer::write` already runs independent 32 KiB SFTP writes
on the shared multithreaded SSH runtime. The missing overlap was source
preparation, rather than one operating-system thread per protocol packet.

## Decision
An upload owns one IO producer that fills and encodes source batches while the
consumer sends the preceding batch. A rendezvous channel limits retention to
the active batch and one future batch, including during connection recovery.
Short and zero-length provider reads are handled before a batch is sent.
Source reads use `runInterruptible`; cancellation joins the producer before
the caller regains ownership of the input stream.

The maximum upload batch is 4 MiB, providing 128 concurrent 32 KiB requests.
Downloads retain their 2 MiB / 64-request window. Upload timeouts still halve
the active batch down to 32 KiB; fast acknowledgements grow it back to 4 MiB.
The original bytes remain available for replaying smaller unconfirmed slices.
Only confirmed bytes advance progress and SHA-256. All acknowledgements drain
before the contiguous offset advances, and publication still requires commit.
The JSON wire limit includes Android's escaped Base64 slashes at the new size.

## Rejected alternatives
Several concurrent JNI writes to one upload would conflict with its contiguous
offset and recovery contract; the native owner serializes these calls anyway.
Opening independent SSH connections would duplicate authentication and session
ownership. Unbounded prefetch would retain the entire file while disconnected.
Increasing the shared runtime's thread count does not remove source/network
serialization and would affect terminal sessions without throughput evidence.

## Consequences
Source preparation overlaps network IO without introducing persistent state,
dependencies, or a second SSH connection. The upload holds at most two source
buffers and their encoded strings; JSON and JNI still require temporary copies.
Source failure and cancellation abort only the owned staging upload. Actual
throughput continues to depend on the provider, device, server and network.

## Validation
The protocol peer withholds acknowledgements until all 128 upload requests
arrive, then reverses the replies and verifies the final bytes. Kotlin tests
block the first write until the next source batch is read, bound speculative
reads, coalesce short reads, commit empty files, interrupt blocked prefetch,
and propagate source failures. Existing recovery tests exercise smaller retries,
window regrowth, lost commit acknowledgements and cancellation during reconnect.

## Supersedes
The upload window and sequential preparation in
[the original transfer decision](2026-10-23-pipelined-resumable-transfers.md).
Its handle, resume and commit ownership remains applicable.

## Revisit when
Measured links need a larger request window, or providers require cancellation
beyond interrupting an active stream read.
