# Pane-owned local SSH forwarding

## Status

Reviewed for integration on 2026-09-26.

## Context

An authenticated native SSH pane needs a small local forwarding control. Opening
another SSH process would duplicate authentication and connection ownership.
A listener must not survive its pane, including normal shell exit, and a pending
creation must not finish after the owner has closed.

## Evidence

- `ssh_session/forward.rs` reuses authenticated pooled sessions and opens
  direct-tcpip channels through the existing SSH runtime.
- The original change cleared established forwards on failure but not normal
  exit; its detached creation task outlived the pane.
- An unrestricted accept loop could create arbitrary numbers of channel tasks
  and bidirectional copy buffers from local clients.
- The original screenshot-only readiness override read the process environment
  from a render-time query and bypassed real SSH readiness in test builds.

## Decision

Keep listeners in the terminal view and the pending GPUI operation in an owned
Task. Exit, connection failure and view destruction release both. The existing
network-to-GPUI bridge cancels its future when the result receiver is dropped;
a stale completion cannot install a forward or show a toast for a dead pane.

Each listener accepts at most 64 concurrent channel tasks. Excess clients wait
in the OS TCP backlog until a slot is available. This bounds task/copy-buffer
cost per listener; it is an engineering limit, not a throughput guarantee.
Completed channels are reaped and failures are logged without killing the SSH
transport shared by the terminal and other forwards.

The endpoints remain loopback-only on both sides. Creating and stopping a
forward requires explicit user action. New labels use typed catalog messages.
Readiness uses actual session state; screenshots cannot enable SSH on local panes.

## Rejected alternatives

- A second SSH process or global forwarding service would duplicate lifetime
  and authentication policy.
- Detached creation cannot guarantee cancellation when the pane disappears.
- Unbounded per-client tasks allow local load to grow memory without a limit.
- Persisted rules, automatic discovery and remote/dynamic forwarding are outside
  this local forwarding capability.

## Consequences

The new behavior adds no persistent thread or timer. UI rendering does not read
files, resolve destinations or inspect environment variables. File/profile
resolution and network I/O remain in the existing background runtime. A forward
owns its channels but never disconnects the shared authenticated transport.

## Validation

Regression coverage exercises real SSH channel exchange, half-close, occupied
ports, channel rejection, listener/connection disposal, and bounded concurrency.
UI regressions click the actual port fields and confirmation/cancel controls,
check invalid input, and reject submission after the pane stops being ready.
Terminal and bridge regressions cover normal exit, failure and pending-work
cancellation. These tests do not claim arbitrary-host throughput or physical
platform visual acceptance.

## Supersedes

None.

## Revisit when

Revisit the limit and UI state model if measured workloads need more concurrent
channels, or when persistent/reconnecting forwards are explicitly introduced.
