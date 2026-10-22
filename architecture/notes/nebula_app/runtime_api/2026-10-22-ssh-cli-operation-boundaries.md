# SSH CLI operation boundaries

## Status

Implemented; pending maintainer review.

## Context

The SSH CLI can use the resident runtime or execute through Pebrel without a GUI.
That fallback must not repeat an operation whose response was lost. Directory
sync also needs one stable destination before deciding whether to remove stale
entries.

## Evidence

The original fallback handled every transport error by executing again locally.
The runtime could therefore complete an operation before a lost response caused
another execution. Exec capture also stopped at EOF before later exit status.
Non-destructive sync used upload destination semantics, while destructive sync
used the supplied path directly; enabling deletion changed the target tree.
Counting raw path components admitted `/var/..` as a destructive destination.

## Decision

Only a missing runtime or failed TCP connection permits headless fallback. Once
submission may have started, transport failures report an unknown outcome and
require state inspection before retry. Exec capture sends stdin EOF and keeps
reading until channel close, including exit status after output EOF.

Both sync modes copy directory contents into the same resolved remote root.
Destructive sync validates that root before creating or writing entries, including
when resolving a missing target through its nearest existing ancestor. Directory
walks preserve empty directories and do not traverse descendant symbolic links.
The CLI resolves local paths before sending them to another process, whose
working directory may differ. File I/O remains owned by the SSH API's file module;
workspace SSH pane creation and retry remain UI orchestration.

## Rejected alternatives

- Retry after any timeout: cannot distinguish a failed operation from a lost reply.
- Validate only the supplied path: ignores dot components and server-side links.
- Reuse upload semantics only without deletion: changes the root when deletion is enabled.
- Follow descendant links: makes the selected tree an unreliable write/delete boundary.

## Consequences

A disconnected CLI may report an unknown outcome even if execution succeeded.
Directory transfers report an error for unsupported symbolic links. Sync with
deletion remains non-transactional: failed uploads stop deletion, but completed
writes are not rolled back. SFTP path checks cannot prevent a concurrent remote
actor from replacing a path between protocol requests.

## Validation

Regression tests exercise a TCP peer that consumes a request and drops its reply,
headless invocation counts, a real SSH codec that sends EOF before status, and an
in-memory SFTP peer for directory round trips, empty directories, stale-entry
removal, link handling, and canonical-root rejection before mutations. Settings
reset and existing workspace navigation/skill controls retain their regression
coverage. Live server acceptance is additional evidence, not a replacement for
these deterministic tests.

## Supersedes

None.

## Revisit when

The protocol supplies durable operation IDs with deduplication, or the remote
filesystem supports a stronger atomic traversal/write boundary.
