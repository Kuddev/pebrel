# Recover SFTP channel setup on a reused SSH transport

## Status

Implemented.

## Context

Pebrel 2.6.0 can report a channel-open failure in both the file browser and
clipboard upload while a newly opened connection works. An authenticated
transport can still be alive when the server refuses another session channel.

## Evidence

The two entry points share SFTP negotiation. In 2.6.0 both returned the first
channel error. A cached browser session also returned its first listing error,
leaving recovery to the next user action.

## Decision

Before requesting the subsystem, the browser can retire a refusing pooled
transport and authenticate on the already resolved route. Pool eviction checks
the transport identity. The original terminal stays connected. Clipboard uploads
belong to a specific pane and retry a transient channel refusal once on its
original connection after 400 ms. Dropping that pane cancels the wait.

An interrupted cached directory listing can reopen its subsystem and repeat the
read at the requested path. Authentication failures, rejected subsystems and
filesystem permissions are reported to the caller.

## Rejected alternatives

Re-resolving a clipboard destination could upload to a different host after
an alias change. Replaying a partially completed file write could duplicate
data. Both recovery points run before any upload bytes are sent.

## Consequences

A permanently exhausted pane transport can still reject the second attempt.
There is no periodic reconnect task.

## Validation

The SSH loopback fixtures exercise a one-time channel refusal and a transport
which refuses every session request. The SFTP protocol fixture checks that a
lost listing is repeated once and access denial is returned.

## Supersedes

None. The pane ownership contract from 2026-10-25 remains in force.

## Revisit when

Revisit if a pinned pane needs transfers on a second independently authenticated
transport or the SFTP client exposes typed close notifications.
