# Pane-owned SSH file intake

## Status

Implemented; native platform and physical clipboard/drag acceptance limits are
recorded in the accompanying PR.

## Context

Local file-tree drops inserted host paths into an SSH terminal; external drops
were rejected. Native file clipboard entries were reduced to text. Remote tools
cannot read those local paths. Issue #553 requests upload followed by remote
path insertion without implicit submission.

## Evidence

`terminal/view/path_drop.rs` previously rejected SSH external paths, while the
file-tree handler in `terminal/view.rs` unconditionally pasted `path_text`.
The native GPUI clipboard provides typed `ExternalPaths` on supported platforms.
Existing `ssh_sftp/transfer.rs` pipelines bounded packets and batches local disk
I/O; file intake uses that implementation instead of buffering entire files.

## Decision

The GPUI adapter owns one pending operation per pane. It accepts native file
payloads, external file drops, and local file-tree drops. Ordinary clipboard text
never authorizes reading a path. Local terminals and WSL retain their existing
path quoting/conversion behavior.

The existing weak, pane-scoped SSH transport is named `PaneConnection` now that
it provides both transcript capture and explicit file intake. A file subsystem
opens on the same authenticated transport. Changing an SSH alias cannot redirect
an upload, and closing a pane does not reconnect to a substitute host. Existing
SFTP browsing keeps its own connection policy.

Each batch creates an exclusive, randomly named directory below the reported
remote cwd, or the SFTP home directory when no cwd is known. Its permissions are
set to 0700 before writing data. This avoids overwriting project files, including
concurrent writers that a name-existence check would miss. Duplicate names within
the batch use the shared KeepBoth resolver; directories use the shared scanner.

Only a complete batch returns paths. The adapter checks the original terminal,
connection identity, foreground owner, focus and input generation before pasting
quoted arguments without Enter or broadcasting to other panes. Typing a
caption is allowed; submission, interrupt, cancellation, focus loss, disconnect
or pane destruction prevents insertion and cancels remaining work.

An unaccepted result owns cleanup on the original SFTP subsystem. Cleanup is
best effort with a 30-second overall budget when the connection has disappeared.
Accepted uploads remain in the remote directory for tools to read later; they
are ordinary user files and are not silently deleted when the application closes.

## Rejected alternatives

- Pasting host paths or falling back to them after an error cannot work remotely.
- Treating path-looking clipboard text as a file would upload without a native
  file-selection action and change ordinary text paste semantics.
- Resolving the current SSH alias again could send files to a newly edited host.
- Writing directly over a cwd filename risks destroying an unrelated project file.
- A separate byte-buffer upload loop would duplicate throughput and cancellation
  rules and consume memory proportional to file size.

## Consequences

Uploaded paths include the batch directory. A read-only target directory reports
an error. Bulk progress and cancellation are shown in the owning terminal.
Cancellation drains in-flight publication work rather than aborting a rename
transaction. A disconnected server can retain partial staging files if cleanup
cannot reach it. No transfer-rate cap, dependency or persisted setting is added.

## Validation

Tests exercise the real SFTP codec for byte integrity, same-name files, directories,
private directory permissions, cancellation, failure and discarded-result cleanup.
GPUI tests cover input-owner changes, focus, session replacement, disconnect,
quoting and insertion without submission. Clipboard tests distinguish native
file entries from path-looking strings. These are not physical Windows/macOS
clipboard or drag acceptance tests.

## Supersedes

None.

## Revisit when

The application supports a user-selected attachment directory or a persistent
transfer queue with explicit completion/cleanup ownership.
