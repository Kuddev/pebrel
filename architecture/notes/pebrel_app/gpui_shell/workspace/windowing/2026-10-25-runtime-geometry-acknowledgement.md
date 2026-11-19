# Runtime geometry acknowledgements follow layout

## Status

Implemented for review.

## Context

Runtime clients may send terminal input immediately after a successful split,
zoom or pane-ratio request. The split tree changes synchronously, but GPUI
normally commits the resulting terminal viewport during the next prepaint.
The PTY message queue must receive that geometry before subsequent input.

## Evidence

The 2.3.0 candidate's [package conformance run](https://github.com/Kuddev/pebrel/actions/runs/40178957933)
observed unchanged PTY widths immediately after a successful ratio change on
Linux and macOS. Later diagnostics showed the requested widths had arrived.
An immediate Runtime input dispatch can therefore overtake deferred prepaint.

`windowing/runtime_resize_tests.rs` reproduces the ordering through the actual
Runtime dispatcher, real split layout and both panes' PTY message channels.
Before this fix it fails because no size message precedes the next input.
Initial split layouts can also encounter the ordinary window-startup grace,
leaving a child temporarily at its unsplit spawn geometry.

## Decision

After a successful structural geometry command, release the workspace borrow
and commit the window's GPUI layout before responding. Prepaint remains the
single owner of viewport measurement and queues each pane's size normally.
Release the draw arena after this explicit draw, as GPUI requires.

An explicit structural resize also bypasses the initial window-size grace.
Ordinary window startup and interactive dragging retain their existing timing.

## Rejected alternatives

- Sleeping or retrying in conformance hides a real ordering contract failure.
- Estimating PTY sizes from the split ratio duplicates font, scale and layout
  measurement, and can disagree with the rendered terminal.
- Flushing every Runtime command adds unnecessary draws to reads and input.
- Removing all startup grace would reintroduce transient initial window sizes.

## Consequences

Successful split, zoom and ratio requests pay for one layout commit before
acknowledgement. Failed requests do not draw. This does not activate the window.
The response guarantees queue ordering; it does not wait for a remote SSH peer
or a child process to finish reacting to its resize.

## Validation

- The dispatcher regression checks both panes, immediate input ordering and
  an invalid target without pumping a frame between requests.
- `structural_layout_during_startup_commits_without_the_window_grace` checks
  immediate PTY sizing and absence of a delayed duplicate.
- `review_regression_quiet_startup_and_maximize_deliver_the_latest_pty_size`
  retains the ordinary startup-grace and latest-window-size controls.
- The package conformance resize assertions remain unchanged.
- The rebuilt Linux ARM64 GPUI binary passes nine native conformance cases;
  the existing SSH-open case remains skipped because Runtime lacks that method.

## Supersedes

None.

## Revisit when

GPUI exposes a layout-only commit or Runtime acknowledgements gain an explicit
asynchronous layout-completion boundary. Preserve the PTY-before-input order
and the ordinary window startup controls when replacing this implementation.
