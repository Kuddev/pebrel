# Cancelled remote directory reads retire their SFTP subsystem

## Status

Proposed during PR #582 review.

## Context

Remote navigation now cancels superseded tasks. The browser previously left its
SFTP session in the cache while awaiting a directory listing.

## Evidence

The pinned russh-sftp 2.3.0 `SftpSession::read_dir` opens a directory, awaits
`readdir` requests, and only then sends `close`. Dropping that future skips the
close. Its raw request timeout is also owned by the dropped future. Keeping the
subsystem alive therefore retains remote handles and potentially pending replies.

## Decision

Move the session out of its cache slot while a listing owns the mutex. Put it back
only after successful completion. Cancellation and errors drop the subsystem;
the next navigation may reopen it through the existing pooled SSH transport.
Successful reads keep connection reuse and unrelated panes remain independent.

## Rejected alternatives

- Ignoring late UI results does not release transport resources.
- Retaining the subsystem after cancellation leaves the dependency's unfinished
  directory transaction reachable.
- Reimplementing SFTP directory requests solely for cancellation adds a second
  protocol owner and still needs pending-request cleanup.

## Consequences

A cancelled listing costs one new SFTP subsystem on the next navigation, not a
new SSH authentication. Native UI fixtures reconnect to owned synthetic peers.

## Validation

The regression drives the actual codec into `readdir`, cancels the owning task,
and checks both cache retirement and eventual server-handler release. It also
checks successful-listing reuse. The pre-fix cache-retirement assertion fails.
Full GPUI and platform validation remain distinct from the focused codec probe.

## Supersedes

The cached-session assumption in the cancellation path described by
`2026-10-10-pane-cwd-following.md`.

## Revisit when

The pinned SFTP client supplies cancellation-safe directory and request ownership.
