# Recycle-bin operations outside the GPUI update

## Status

Proposed in the fix for issue #924.

## Context

The file-tree confirmation invoked `SHFileOperationW` synchronously inside a
workspace update. The Windows Shell can pump messages while handling a locked
file, re-entering GPUI while its application `RefCell` is already borrowed.

## Evidence

[Issue #924](https://github.com/Kuddev/pebrel/issues/924) reports deleting a database
held open by a service. Its screenshot identifies `gpui/src/app/async_context.rs`
at `AsyncApp::update`, with `RefCell already borrowed`. The confirmation callback
held the workspace update across the native operation.

## Decision

Dispatch the synchronous file operation through the existing background executor.
The task owns the requested path; only its result returns to the foreground.
Use the existing weak workspace handle so closing the window prevents a late
result from updating a destroyed view. Refresh the tree after success and retain
the existing localized failure notice on error.

Initialize and uninitialize the worker's COM apartment around the synchronous
Windows Shell call. Balance both successful COM initialization return codes;
report initialization failure before invoking the Shell.

## Rejected alternatives

- A foreground async task still executes the native call on the UI thread and
  permits its nested message loop to re-enter GPUI.
- Catching a panic leaves the reentrant borrow and UI blockage in place.
- Permanently deleting the file would change the Windows recycle-bin contract.

## Consequences

The confirmation closes while the operation runs. A confirmed deletion finishes
even if the workspace closes; its result is then discarded. The native call is
synchronous within one worker poll, keeping COM initialization on one thread.
Sharing and lock violations use the operating system's error text.

## Validation

The Windows regression holds a real file with sharing disabled, runs the recycle
operation on a fresh worker, and checks the error and retained file contents.
Native GPUI message-loop acceptance is tracked in the PR's validation record.

## Supersedes

None.

## Revisit when

The shared native file-operation adapter gains a cancellable asynchronous API.
