# Completion history locks and cooperative cancellation

## Status

Implemented; native desktop acceptance remains separate from automated checks.

## Context

Completion will acquire more context-dependent sources. The existing GPUI
adapter holds one shared mutex while the engine scans files, and command
submission acquires that same mutex to record history. A slow directory can
therefore delay the foreground even though calculation runs in the background.
Discarding an obsolete UI result also does not stop an already-running scan.

## Evidence

- `nebula_app/src/gpui_shell/terminal/suggest.rs` owns shared history and the
  calculation adapter; `commit_line` uses the same history authority.
- `nebula_app/src/display/suggest_engine.rs` calls history matching before
  directory and filesystem completion for both presentation adapters.
- `nebula_app/src/directory_history.rs::hint` used to retain its database lock
  during filesystem metadata queries.
- `nebula-completions/src/file.rs` enumerates directories recursively, so a
  request can keep doing work after its view no longer accepts the result.

## Decision

Give the GPUI history its own mutex. The shared engine accepts either a borrowed
history for the legacy adapter or a mutex-protected history for GPUI. Return only
owned matching text across that lock boundary: one suffix or at most eight
history candidates. Do not clone the full history for each keystroke.

Directory hints copy their existing maximum of twelve ranked paths and release
the database lock before querying filesystem metadata. Matching order and
persistence stay unchanged.

The GPUI pending task owns a shared cancellation flag. Replacing or dropping the
task, including a mode change, sets the flag before releasing the task handle.
History matching, directory hints and recursive file completion check it between
units of work. The existing query/environment/style checks remain authoritative
for applying a result. File completion discards partial output on cancellation.

The existing file-completion entry point remains available for synchronous
callers. Both it and the cancellable entry point call the same implementation.
No new dependency, worker pool, OS-specific policy or persisted format is added.

## Rejected alternatives

- Keep a global lock and only move work to a background executor: foreground
  submission still waits for that lock.
- Copy every history entry before every request: unnecessary allocation and
  copying when only a handful of matched strings are needed.
- Only discard stale results: already-running obsolete scans keep consuming
  resources until they finish.
- A separate completion implementation per operating system or renderer:
  cancellation and matching rules have one authority.

## Consequences

The change removes filesystem enumeration from the command-history critical
section. It does not claim all foreground persistence is asynchronous or every
lock is contention-free. An individual OS directory/metadata read cannot be
interrupted by the cancellation flag. Shared remote directory fetches keep their
existing owners and can still populate a cache after one consumer goes away.

This is preparation for semantic completion; command grammar, dynamic Git
arguments and project-script discovery are not introduced here.

## Validation

Existing test modules cover real directory enumeration cancellation, history
scan cancellation, history/database lock availability during filesystem work,
and mode replacement notifying an obsolete GPUI task. Existing completion-mode
and environment-isolation regressions remain in place. These tests exercise the
same implementation on native Windows, macOS and Linux CI; automated checks do
not establish manual desktop acceptance.

## Supersedes

Extends `2026-09-29-completion-request-ownership.md`: file operations already in
progress remain non-interruptible, but enumeration now stops between operations.

## Revisit when

Measured matching cost or new data providers require bounded task admission,
snapshot indexing or cancellation of independently owned remote requests.
