# Settled terminal geometry and complete frames

## Status

Accepted for this fix.

## Context

The reported symptom was early session history appearing again and scrolling
to the latest content on entry, tab reattachment and keyboard dismissal. A
remote full-screen application may replay its transcript when its PTY changes
size. The client must avoid requesting a replay for every animation frame and
must honor an application's synchronized-output boundary.

## Evidence

`TerminalViewportTest` on the previous implementation sent all 18 intermediate
IME heights (13 through 30 rows) to the transport. It also observed a redundant
resize immediately after opening at the already measured dimensions. Its two
regressions failed before the change and pass with a single final resize and
no redundant resize, respectively.

The pinned native parser applies DEC 2026 output directly to its grid. Its
snapshot API does not suppress publication during that mode; the upstream
desktop renderer checks the mode separately. A Linux ARM64 probe using the
production JNI adapter observed cursor row 23 before a synchronized redraw,
row 0 during its incomplete history replay, and row 23 after completion.
Resizing the native terminal also ends synchronized-output mode.

The mobile session published a snapshot after each output burst without
checking that mode. `TerminalFramePublicationTest` reproduced partial history
publication both while visible and when reattaching a view. The shared desktop
core already buffers these updates; its existing split-sequence snapshot test
passes. These findings do not establish a new desktop regression or identify
which release first made the user's remote application replay its history.

## Decision

The session owns a conflated geometry mailbox. At startup it allows 120 ms
for measurement, then waits for a 120 ms quiet interval after geometry changes.
Later resize requests use that same quiet interval. Only a changed, settled
size is committed, first to the parser grid and then to the transport. Both
local and SSH transports follow this ordering. No new thread or dependency
is introduced; parsing and geometry still use the existing serial dispatcher.

Frame publication checks the parser's DEC 2026 mode on that dispatcher. It
retains the last complete immutable frame during synchronized output, while
reading and parsing continue. One owned one-second deadline releases an
unterminated update even if the peer stops sending bytes. Normal completion
cancels that deadline; transport termination releases the final frame and
session closure cancels outstanding work. An inactive view does not snapshot
on every output chunk.

## Rejected alternatives

- Scroll to the bottom on every frame or attachment: this would discard a
  user's deliberate history position and cannot prevent remote replay.
- Change server configuration or cancel remote copy mode on attachment:
  neither is justified by the available reproduction.
- Increase a delay on every output chunk: ordinary streaming output should
  remain immediate, and timing alone is not a complete-frame boundary.
- Resize the local grid on every IME frame but debounce only the peer: native
  resize itself clears synchronized-output mode and would expose partial output.

## Consequences

During geometry animation, the view displays the previous frame at its old
dimensions until the size settles. Headless startup without a measurement has
a bounded 120 ms wait. Continuous resize animation may defer the commit until
it stops; transport output and input are not paused. A peer that omits an end
marker may expose incomplete output after one second instead of freezing.

The change does not persist or clear history, recreate the transport on tab
selection, replay stored output, or change the server's session state. It does
not hide unmarked transcript output or establish the cause of every reported
Windows symptom.

## Validation

- JVM session tests cover animated heights, identical final sizes, close during
  a pending resize, partial replay, reattachment, silent-source timeout and EOF.
- The native engine test covers split begin/end markers, explicit timeout
  release and the native resize reset through the production JNI API.
- The shared desktop split-sequence snapshot regression is run on Linux.
- Actual Android IME animation and Windows user interaction still require
  device verification; JVM/native-host probes are not device acceptance.

## Supersedes

None.

## Revisit when

The native API supplies immutable synchronized snapshots or a measured case
requires explicit IME animation lifecycle information in place of settling.
