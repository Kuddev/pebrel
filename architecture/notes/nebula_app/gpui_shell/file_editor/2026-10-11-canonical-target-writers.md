# Independent local drafts coordinate canonical-target publication

## Status

Proposed. Validation results and platform limits are recorded in the PR.

## Context

Separate editor windows and draft copies can hold the same loaded bytes for one
local file. Each draft must retain its loaded-byte precondition when saving.

## Evidence

Document::save checked current bytes, staged a sibling temporary file, checked
bytes again, then called persist. Two writers could both complete that last
check before either replaced the target. The later replacement could then
overwrite a different draft despite having verified the old bytes.

## Decision

One active permit belongs to each stored canonical target. Its lifetime spans
both checks and publication. Acquisition only holds a short in-memory mutex;
it never waits for the other write or holds that mutex during filesystem work.
A contender returns a typed busy result and keeps its draft. Once the permit
is released, a retry still compares the loaded bytes and may report a conflict.
Every success/error path releases its permit, so the registry only contains
active writes and cannot retain abandoned per-file entries.

## Rejected alternatives

- A process-wide publication lock unnecessarily serializes unrelated files.
- Waiting synchronously for a writer can block the editor's executor.
- Locking the requested path separates symlink aliases for the same target.
- Updating the loaded revision after a failed save loses conflict evidence.

## Consequences

This coordinates this application's writers, including independent loads and
symlink aliases. It does not claim a transactional CAS against arbitrary external
processes. Existing byte, symlink, permission and atomic replacement checks stay
in their current authority. Remote publication has a distinct SFTP transaction.

## Validation

A real local publication is gated after its last check while another independently
loaded draft attempts a save. Tests verify busy, release, retry/conflict, exact
persisted bytes, symlink identity and real GPUI draft/notice behavior. All files
are isolated temporary fixtures; no user document or native clipboard is used.

## Supersedes

The assumption that two final byte checks alone coordinate in-process writers.

## Revisit when

The local write backend provides a platform transaction or document ownership
moves to a shared persistent editor service.
