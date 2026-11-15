# Confirmed deletion in the remote Files drawer

## Status

Proposed with the desktop SFTP deletion change.

## Context

The GPUI Files drawer can select and transfer remote items. The old shell has
a recursive delete operation, while Android removes files, symlinks and empty
directories. The GPUI drawer now needs a deletion action for its selection.

## Evidence

A pane and a destination are separate identities: panes that share an SSH
connection can show different directories. The drawer also caches its last
listing during tab switches. An SFTP batch can remove earlier entries before a
later request fails, and a lost reply leaves that request's outcome unknown.

## Decision

The dialog captures the selected entries and the browsing generation. On
confirmation, the target must still be current. The pane's SFTP adapter checks
that the entries are direct children of the listed directory, compares the
available metadata with the listing, and sends `REMOVE` for files and links
or `RMDIR` for directories. It stops at the first error. SFTP v3 metadata is
not a unique identity or an atomic condition on the subsequent removal.

After the operation settles, the drawer reloads the affected directory if
it's still visible and marks matching parked listings for reload. A navigation
to another directory remains in effect. The workspace owns the outstanding
task, and the adapter retires the subsystem if a request fails or is dropped.

## Rejected alternatives

- Recursive traversal gives a single confirmation authority over unlisted
  contents; the new action follows Android's empty-directory boundary.
- Interpolating paths into a remote shell command changes filename semantics
  and requires a shell on a connection that may only offer SFTP.
- Retrying deletion after a disconnect could remove a replacement at the
  same path. Refreshing shows what the server currently lists.

## Consequences

A batch is not transactional. The refreshed list shows remaining entries
after partial failure. No stored settings or SSH protocol extensions change.

## Validation

The remote Files GPUI fixture exercises the confirmation controls against an
owned SFTP peer, including cancel, failure after a successful removal and a
directory change while confirmation is open.

## Supersedes

None.

## Revisit when

A supported SFTP protocol operation can make deletion conditional on an
entry identity, or the product adopts a separate recursive delete workflow.
