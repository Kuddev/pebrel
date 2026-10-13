# Read remote transcripts through the owning SSH connection

## Status
Implemented; targeted Rust and real-OpenSSH file-capture checks passed.

## Context
Desktop SSH panes already receive authenticated Hook identities, but conversation
reads rejected every SSH source. Reading a remote absolute path on the desktop,
or opening a new connection by hostname during a delayed read, would target the
wrong environment or lose the original pane's ownership boundary.

## Evidence
- `TerminalView::handle_ai_hook` records the provider's native session/file identity.
- `ssh_session::transcript` exposes an opaque reader for one live shell lifetime.
- `assistant_answer/conversation/remote.rs` and `remote_read.py` capture bytes;
  the existing shared native-record projector validates and renders them.

## Decision
- Publish the reader only after the SSH shell is opened. Capture the exact
  authenticated transport with a weak reference and a per-pane close signal.
- Never resolve a hostname, reauthenticate or select a replacement pooled
  connection from this read capability. Closing the shell cancels its reads.
- Send a fixed Python read program over a separate exec channel. Arguments are
  encoded as JSON/base64, never inserted as shell source or into the live PTY.
- Bound the header/page and total duration; verify the opened file and path
  metadata before returning bytes. Detect replacements/truncation as changes.
- Keep reads on the background executor and check native conversation identity
  again on the UI thread before publishing the page or enabling input.

## Rejected alternatives
- Treating a remote path as a host-local path.
- Reusing the general authenticated-connect helper: it can open another
  connection or prompt for credentials after the original pane has ended.
- Guessing the newest conversation from a directory or terminal screen text.

## Consequences
The remote host needs Python 3 and a Hook-reported native transcript path.
Missing metadata is reported as unavailable, not substituted with another Agent.
This capability serves desktop-owned SSH panes projected to mobile. It is not a
claim that mobile-direct SSH now has its own Hook installer or conversation index.

## Validation
Rust conversation tests cover record/session validation and remote response
bounds. A reader-lifetime test rejects ended/missing transports without opening
a replacement. Six real-OpenSSH checks covered UTF-8/quoted paths, bounded tail
and header reads, earlier pages, out-of-range cursors and invalid file types.
Full desktop-SSH-to-phone Agent interaction remains a distinct device acceptance.

## Supersedes
None.

## Revisit when
Mobile-direct SSH adopts the same Hook/record contract, or providers expose a
structured native transcript API that replaces bounded file capture.
