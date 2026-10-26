# Clearing command history and pending suggestions

## Status

Proposed in the fix for #689.

## Context

Pebrel records local, WSL and SSH command history in three files and keeps one
shared in-memory index per resident process. There was no user-facing removal
operation. Removing files alone leaves old suggestions in that index and in
individual panes, including results still being calculated on workers.

## Evidence

- [History removal and temporary-directory tests](../../../../nebula_app/src/nebula_history.rs)
  exercise all categories, missing files and partial filesystem failures.
- [Completion requests](../../../../nebula_app/src/completion.rs) share the history
  mutex and provide a revision that can be read without initializing disk state.
- [Settings interaction tests](../../../../nebula_app/src/gpui_shell/settings_pane/command_history/tests.rs)
  exercise confirmation, cancellation, busy state, failure and origin closure.
- [Terminal lifecycle tests](../../../../nebula_app/src/gpui_shell/terminal/view/startup_tests.rs)
  cover cross-window invalidation and cancellation of pending calculations.

## Decision

The confirmed action removes Pebrel's three command-history files on a worker,
under the existing shared history lock that serializes recording. A category's
in-memory index is removed only when file removal succeeds or the file is already
absent. All failures are returned, including partial success. Saved commands,
directory history and the shells' own history are outside this action.

The application owns the operation through completion. A global presentation
state prevents duplicate actions across windows and survives closing Settings.
Terminal views own subscriptions that clear ghost text, popup entries and editor
queries, and cancel pending calculations. Queries capture a history revision;
publication also rejects results produced before a clear, including partial clears.
The revision check does not load history during rendering. It is incremented
before releasing the history lock.

## Rejected alternatives

- Deleting only files: loaded indexes and in-flight results revive old suggestions.
- Resetting every application setting: exceeds the requested history operation.
- Per-command tombstones: retain removed command text and change persistence for
  an operation that can use ordinary file removal.
- Making the originating pane own completion: closing it can strand other panes
  with stale suggestions and leave another Settings window busy indefinitely.
- New cross-process locking or a persistent worker service: unnecessary for the
  existing resident application's shared history owner and adds keystroke-path cost.

## Consequences

The schema and normal recording path remain unchanged. Commands recorded after
removal can create a new history file. Independent resident processes do not share
in-memory indexes; their live caches are not governed by this application's GPUI
subscriptions. Removal takes the existing history mutex around three filesystem
operations, so recording can briefly wait for them on a slow data filesystem.

## Validation

Filesystem tests use temporary directories only. UI workers are controlled by
test-only futures and never call production history removal. Native visual and
platform acceptance are separate from these headless interaction tests.

## Supersedes

None.

## Revisit when

History ownership becomes cross-process, or users need selective entry deletion
instead of clearing the whole Pebrel command history.
