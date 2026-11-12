# Keep shell output draining during SSH writes

## Status

Proposed for review.

## Context

The desktop SSH shell pump selected between terminal input and SSH output, but
awaited an entire channel write inside its input branch. SSH writes can wait for
remote window credit. While waiting, the pump stopped consuming channel output.

## Evidence

Russh delivers channel output through a bounded receive queue. When a shell echoes
large input while the send window is exhausted, that queue can fill and prevent
the connection from processing the window adjustment needed by the pending write.
The loopback regression in [lifecycle tests](../../../../nebula_app/src/ssh_session/lifecycle/tests.rs)
uses a 4096-byte peer window, 512-byte packets and a large echoed input. With the
old pump it hits the fixture's ten-second timeout; the duplex pump completes.
This establishes a backpressure defect, not a universal explanation for typing
latency or a claim about real-network throughput.

## Decision

[The shell lifecycle](../../../../nebula_app/src/ssh_session/lifecycle.rs) owns the
channel reader and writer separately. The existing pump keeps at most one pending
write future and polls it alongside output, synchronized-output deadlines and
disconnect handling. Later input and resize requests wait for that write, retaining
their ordering. Owned input bytes go directly to the channel writer.

The existing channel guard still owns close-on-drop, including startup failure,
cancelled metadata requests and pane shutdown. No background writer task or extra
queue is introduced.

## Rejected alternatives

- Increasing the SSH window only postpones the same receive-queue stall.
- A detached writer task adds another cancellation owner and input queue without
  being necessary for concurrent polling.
- Dropping a partial write whenever output arrives risks lost or duplicated input.
  The pending future remains owned by the pump until completion or session teardown.

## Consequences

Incoming data and control messages continue to advance while a write is pending.
One write remains in flight, preserving FIFO input and resize semantics. On pump
exit, the pending future is dropped and the channel guard closes the channel.
SFTP scheduling and terminal rendering policy are unchanged.

## Validation

The new loopback regression times out with the old implementation and passes with
the change. The shell lifecycle suite passes 21 tests; the existing test requiring
an external SSH fixture remains ignored. That suite also covers input replies,
resize, startup failures, cancellation and preserving output on disconnect.
Windows user interaction and wide-area throughput still require native testing.

## Supersedes

None.

## Revisit when

The channel library changes its flow-control or cancellation contracts, or input
scheduling needs more than a single ordered write in flight.
