# SSH file-copy paste ownership

## Status

Proposed for review with [#553](https://github.com/Kuddev/pebrel/issues/553).

## Context

The GPUI clipboard path accepts text or images. Native file copies can therefore
insert a Windows filename that a remote program cannot read. Image upload already
returns an SSH-accessible path, while the file browser's upload command owns browser
state and does not return pasteable paths to a terminal input lifetime.

## Evidence

- `platform/clipboard_files.rs` recognizes the native Windows CF_HDROP format.
  `clipboard/files.rs` validates its owned Unicode payload, file count and bounds.
  It does not infer files from text, including text that looks like a local path.
- `terminal/view/file_paste.rs` captures the originating terminal, SSH destination
  and command identity before starting work. Focus loss cancels the operation,
  including a switch away and back before the upload completes.
- `ssh_session::SftpSource` captures a weak reference to the pane's original
  authenticated connection and a gate that closes when its shell owner ends.
  Alias/profile changes cannot choose another connection, and the transcript
  reader remains a separate read-only capability using the same lifetime gate.
- `ssh_sftp/clipboard_files.rs` opens a dedicated SFTP channel on that connection.
  Files are opened with CREATE + EXCLUDE and private permissions; existing remote
  files cannot be truncated by the upload path.

## Decision

The platform adapter copies and releases the clipboard data before decoding.
The paste action accepts at most 32 ordinary files with distinct names; directories
use the existing Remote Files workflow. Only native Unicode file-copy payloads on
Windows enter this path. Local shell, WSL, other-platform, text and image pastes
retain their existing behavior.

The SSH operation never resolves the destination or credentials again. It resolves
the captured pane directory through SFTP, falling back
to that same session's home (`.`) only when the pane has no known remote directory.
It holds local file descriptors, validates every source before creating remote
files, and streams through the existing bounded transfer engine. A changed local
size or modification time invalidates the batch. Same-size edits that preserve the
timestamp are not detected; this is not a filesystem snapshot guarantee.

One upload result owns all files it created. A later file failure rolls back the
batch. A successful result remains unaccepted until the original pane still has
the same terminal, an open original SSH scope, command identity and focus. Only then are all
paths POSIX-quoted and inserted through the non-broadcast paste path, with no Enter.
Dropping an unaccepted result schedules cleanup on its original SSH runtime and
channel, even if a host alias has since changed.

The pane owns cancellation and its waiting UI task. Typing, hiding the pane,
losing focus, closing the session, Cancel or Esc requests cancellation. A compact
status indicator remains visible until the upload worker resolves. Streaming can
be interrupted; an in-flight exclusive OPEN retains its bounded SFTP timeout so
the acknowledgement can establish whether the new file belongs to this operation.
Cleanup has a finite budget and cannot promise removal after a broken connection.

## Rejected alternatives

- Treating path-like text as a file request would upload text the user intended to
  type, and makes clipboard contents depend on local filesystem state.
- Reusing the file-browser controller as the terminal task owner would couple
  target selection and cancellation to another pane's navigation.
- Overwriting the original basename would silently change remote project data.
  This operation reports a conflict instead; the browser remains the explicit
  overwrite workflow.
- Reusing image upload's `/tmp` destination would disregard the requested pane
  directory. Resolving SFTP `.` only when cwd is unknown keeps the fallback tied
  to the authenticated account, without guessing a different session's cwd.

## Consequences

No new dependency, setting, credential format or Agent-specific protocol is added.
The native feature is Windows-only; directory copies and legacy ANSI CF_HDROP
are unsupported. Up to 32 local descriptors plus one transfer buffer are retained.
New remote files remain in the chosen directory after their paths are accepted.
Failed cleanup is logged and may require manual removal; existing files are never
part of the cleanup set. The feature does not execute uploaded content.

## Validation

Regression suites exercise bounded/malformed native payloads, Unicode filenames,
the real SFTP codec, byte integrity, private file permissions, cwd/home selection,
name collisions, partial failures, source mutation and cancellation. A loopback
SSH regression replaces the connection-pool entry and closes the original pane
scope during exclusive OPEN, checking endpoint ownership and rollback. GPUI tests
exercise the actual Cancel control and Escape key, late results, focus changes,
session replacement, text preservation, quoted input and absence of broadcast or
Enter. The Ctrl+right-click menu regression observes terminal focus at its Paste
input event, before dismissal restores focus; native files use that same explicit
Paste entry. Compile/test execution and native Explorer acceptance are reported in the
PR; the presence of these tests alone is not a claim of execution or a live SSH
deployment check.

## Supersedes

None.

## Revisit when

Another platform exposes native file-copy payloads, directory paste is requested,
or server capabilities can guarantee atomic batch publication and stronger cleanup
ownership. Those changes should keep the same source-pane acceptance boundary.
