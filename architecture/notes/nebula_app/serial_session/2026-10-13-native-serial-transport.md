# Serial panes use native OS serial APIs and the standalone message protocol

## Status

Implemented; pending review.

## Context

Issue #524 asks for COM/serial connections. Pebrel had no serial transport. SSH panes already
show that a pane needs no PTY: `EventLoopSender::standalone` plus `Msg::Input/Resize/Shutdown`
and `StreamProcessor::feed` drive the same terminal view.

## Evidence

- `nebula_app/src/ssh_session/lifecycle.rs` is the existing non-PTY transport.
- The existing native dependencies provide the small API surface required here; port enumeration and flow-control negotiation are outside this feature.
- `libc` and `windows-sys` are already dependencies; only the `Win32_Devices_Communication`
  feature of `windows-sys` is added (no lock change).

## Decision

`serial_session` opens the port with termios (Unix) or `GetCommState`/`SetCommState` (Windows),
then runs one reader thread and one writer thread over cloned handles. Both stop through one flag.
Unix descriptors remain nonblocking and use 20 ms readiness waits. Windows uses 20 ms read and
100 ms write timeouts. The writer returns to the command queue between partial writes; a drop guard
also stops the reader if writer-thread creation fails.
The destination is the string `serial:<port>[,<baud>[,<data><parity><stop>]]` (default 115200 8N1).
The existing host editor validates serial destinations with the transport parser before saving;
its default SSH username and port are not attached to the serial address. SSH proxy/jump options
remain SSH-only, and its connection-test button explains how to open a saved serial connection.

The shared `ssh_profiles/host_lists.rs` codec escapes commas and percent signs in serial entries
for the comma-delimited saved/pinned/hidden lists. Both shells use this codec; ordinary SSH aliases
retain their previous byte representation. Profile JSON and session JSON retain the raw address.
Session restoration dispatches through `LaunchSession::Serial`. Serial panes have no `ssh_destination`,
so SFTP, remote files and SSH completion are never attached to them.

## Rejected alternatives

- `serialport` crate: extra dependency surface for this limited set of native operations.
- Blocking Unix `write_all`: a full device buffer prevents reading Shutdown and retains both handles.
- Accepting serial addresses in the SSH destination validator: would also admit serial jump hosts.
- Saving raw commas in the host list: reload splits one serial address into multiple hosts.
- A single thread alternating read and write: input waits for the read timeout.
- Reusing `LaunchSession::Ssh` with a `serial:` host: restore, duplicate and tabs would treat it as SSH.

## Consequences

Unix accepts only standard rates up to 230400; Windows accepts any rate the driver does. Flow control
is off. On Windows a pending read blocks writes on the synchronous handle, so reads time out after 20 ms.
Telnet and a dedicated settings form are not part of this change.

## Validation

Parser and Unix PTY regressions live in `serial_session/tests.rs`: cancellation under backpressure,
sender disconnection, ordered partial writes, and peer disconnection. The GPUI editor regression
clicks Save with the default SSH username, reloads the host list, checks pin persistence, and reopens
the editor; invalid baud rates must not overwrite the saved connection. The codec has a roundtrip
test including literal percent sequences. Native CI compiles both platform adapters.
No physical serial device or Windows driver loopback has been tested; PTY evidence does not prove
hardware handshake behavior.

## Supersedes

None.

## Revisit when

A baud rate above 230400 or hardware flow control is required, or a settings form replaces typed addresses.
