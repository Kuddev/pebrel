# Serial panes use native OS serial APIs and the standalone message protocol

## Status

Implemented; pending review.

## Context

Issue #524 asks for COM/serial connections. Pebrel had no serial transport. SSH panes already
show that a pane needs no PTY: `EventLoopSender::standalone` plus `Msg::Input/Resize/Shutdown`
and `StreamProcessor::feed` drive the same terminal view.

## Evidence

- `nebula_app/src/ssh_session/lifecycle.rs` is the existing non-PTY transport.
- `Cargo.lock` is built with `--locked` in CI; a new crate would need a hand-edited lock file.
- `libc` and `windows-sys` are already dependencies; only the `Win32_Devices_Communication`
  feature of `windows-sys` is added (no lock change).

## Decision

`serial_session` opens the port with termios (Unix) or `GetCommState`/`SetCommState` (Windows),
then runs one reader thread and one writer thread over cloned handles. Both stop through one flag.
The destination is the string `serial:<port>[,<baud>[,<data><parity><stop>]]` (default 115200 8N1).
It carries no whitespace or shell metacharacters, so it is stored as an entry in the existing
SSH host list and restored through `LaunchSession::Serial`. Serial panes have no `ssh_destination`,
so SFTP, remote files and SSH completion are never attached to them.

## Rejected alternatives

- `serialport` crate: extra dependency tree and a lock edit for what is a few syscalls.
- A single thread alternating read and write: input waits for the read timeout.
- Reusing `LaunchSession::Ssh` with a `serial:` host: restore, duplicate and tabs would treat it as SSH.

## Consequences

Unix accepts only standard rates up to 230400; Windows accepts any rate the driver does. Flow control
is off. On Windows a pending read blocks writes on the synchronous handle, so reads time out after 20 ms.
Telnet and a dedicated settings form are not part of this change.

## Validation

Parser unit tests in `serial_session/tests.rs`; platform I/O is verified through CI builds on each OS.
No hardware loopback test is run in CI.

## Supersedes

None.

## Revisit when

A baud rate above 230400 or hardware flow control is required, or a settings form replaces typed addresses.
