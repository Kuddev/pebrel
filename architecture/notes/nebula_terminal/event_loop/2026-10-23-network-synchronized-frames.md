# Network-delivered synchronized frames

## Status

Accepted for implementation; native host and device acceptance remain separate
from the transport regression tests.

## Context

The desktop stream processor is shared by local PTYs and built-in SSH on every
desktop host. Its parser buffers DEC 2026 output, so animation snapshots continue
to see the last complete grid. The parser's default 150 ms watchdog was designed
for short local writes. Network delivery can leave longer gaps within one redraw.

## Evidence

On base `76e3e3de`, a real loopback SSH test sends a complete screen, then a
synchronized update containing synthetic early history, waits 300 ms and sends
the final screen and end marker. Before the end marker, the grid already contains
`EARLY HISTORY` instead of `COMPLETE`. No user transcript or credentials are used.
See `ssh_delayed_synchronized_redraw_keeps_the_complete_screen_visible` in
`nebula_app/src/ssh_session/lifecycle/tests.rs`.

An isolated remote multiplexer 3.4 fixture also emits redraws without outer
synchronization markers. The watchdog change does not make those redraws atomic,
and this evidence does not establish every cause of the reported Windows lag.

## Decision

Use the parser's supported timeout adapter with a one-second watchdog in the
shared stream processor. A complete end marker publishes immediately; ordinary
output and input incur no new wait. Keep the parser's existing repeated-begin
renewal and approximately 2 MiB forced-flush semantics. A silent source missing
its end marker therefore releases the grid one second after the last begin.

At SSH pump completion, flush any unfinished synchronized output before reporting
exit or failure. Keep normal close, remote disconnect and input-channel shutdown
on this same path. Dropping the entire pane cancels its pump and does not require
publishing a final frame to a destroyed view.

## Rejected alternatives

- Delaying every output read: adds latency to normal typing and still cannot
  infer the end of arbitrary unmarked redraws.
- Changing the dependency or copying its parser: the existing timeout trait is
  sufficient and preserves marker parsing and the memory bound.
- Scrolling to the bottom on every repaint: destroys intentional history reading
  and does not prevent intermediate redraws from becoming visible.
- Treating the reproducer as proof of all Windows symptoms: unmarked output and
  actual native rendering require separate investigation and acceptance.

## Consequences

A broken producer that stops mid-update may hold its last complete screen longer
than before. The watchdog and parser cap bound silent-source and large-output
recovery respectively. Repeated begin markers intentionally renew the watchdog,
as in the upstream parser; this is not an absolute lifetime cap.
There is no new dependency, thread, per-byte timer, or screen copy.

## Validation

The SSH regressions cover a 300 ms gap, input during an unfinished redraw,
immediate end-marker publication, silent-source recovery and final output on
exit, unexpected disconnect and shutdown. Core tests cover split markers,
independent animation snapshots and the existing buffer cap. Native Windows
validation is recorded on the corresponding PR, separately from Linux execution.

## Supersedes

None. The Android publisher has a different parser and frame lifetime; its
settled-geometry decision still applies independently.

## Revisit when

Change the watchdog if representative network traces require another bound, or
if the parser changes its synchronization, repeated-begin or overflow semantics.
Do not extend it to unmarked output without a separate protocol and latency case.
