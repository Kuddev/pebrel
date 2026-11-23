# SFTP selection and batch transfer ownership

## Status

Proposed with the SFTP sidebar interaction update.

## Context

The remote sidebar previously retained one selected path. Adding Ctrl/Shift
selection also requires downloads, remote copies and native file drags to carry
the complete selection through their existing asynchronous handoffs.

## Evidence

`SftpController::start_job` advances the controller generation and retires the
previous job. Calling the single-entry transfer once per selected row would
therefore cancel earlier entries. Native export likewise owns one download plan
and one temporary-directory lease for the lifetime of the drag.

## Decision

The browser retains path-based selections, anchors and focused rows in its
existing per-pane snapshots. Display order supplies range selection. A refresh
retains surviving paths; changing directories clears the selection. Mouse-down
on an already selected row defers collapsing the group until a click is confirmed.

Actions capture the ordered entries and existing pane/destination/generation
target before opening a picker or confirming conflicts. One controller job plans
the complete download or copy batch, using the existing concurrency and entry
limits. Native export receives the same batch. Duplicate download names reserve
the other incoming roots before choosing a replacement name.

Keyboard commands belong to the remote list's focus context. Hover actions stop
their gestures from reaching the row's selection and double-click navigation.

## Rejected alternatives

- Starting a controller job per selected row cancels earlier jobs.
- Storing row indices selects different files when a listing is replaced.
- Changing the local tree's selection model expands this remote-only change.

## Consequences

No persisted format, authentication policy or worker model changes. The existing
transfer target validation still rejects results from stale pickers and dialogs.
Selection stays independent of the batch already being transferred.

## Validation

The supplied HTML was inspected in Chromium, including hover and range selection.
Formatting and architecture checks cover the source change. Native compilation,
application visual acceptance and regression tests were not run at the user's request.

## Supersedes

None.

## Revisit when

The browser supports selection across directories or concurrent transfer jobs.
