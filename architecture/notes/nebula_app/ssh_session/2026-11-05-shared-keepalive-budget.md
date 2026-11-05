# Share the SSH keepalive budget between built-in and OpenSSH launches

## Status

Accepted (#816).

## Context

An idle built-in SSH pane ended with "SSH connection ended without an exit
status" after hours, while a parallel standalone OpenSSH client session to the
same host stayed connected. The managed OpenSSH launch has used
`ServerAliveInterval=30` / `ServerAliveCountMax=6` since v0.7.0 to survive NAT
idle timeouts. The built-in russh client instead used a 15 s interval with a
maximum of 3. russh counts every unanswered timer tick and fails the transport
with `KeepaliveTimeout` once the count exceeds the maximum. A server or path
that is silent for about one minute therefore ended the pane.

## Evidence

A scratch russh 0.62.2 client with the old 15 s × 3 policy lost its channel
after about 60 s when the server was stalled for 90–100 s. Servers were stalled
with SIGSTOP on a local macOS sshd and with `docker pause` on an Ubuntu 22.04
OpenSSH 8.9 container. OpenSSH with default settings survived the same stall
and echoed afterwards. Forcing a rekey every three seconds against OpenSSH 10.2
and 8.9 kept the session alive, which rules out key re-exchange as the cause.

## Decision

`ssh::SERVER_ALIVE_INTERVAL` (30 s) and `ssh::SERVER_ALIVE_COUNT_MAX` (6) are
the single keepalive budget. The OpenSSH arguments and the russh
`keepalive_interval` / `keepalive_max` are both derived from these constants.
A path that is silent for up to about 3.5 minutes no longer ends the pane, and
probes still refresh NAT state. The client handler logs the russh disconnect
reason. Transport errors are logged at warning level so that a future report
can distinguish a keepalive timeout from a socket error. A received disconnect
is logged at info level.

## Rejected alternatives

- `keepalive_max = 0` (probes never fatal): on Unix, a dead link would be
  detected only after the TCP retransmission timeout of about 15 minutes.
- Removing probes, which matches OpenSSH defaults: this would regress the
  v0.7.0 fix for NAT idle drops.
- Honoring user ssh_config `ServerAlive*` through `ssh -G`: `-G` prints 0
  whether or not the value was set, so explicit configuration cannot be
  detected.

## Consequences

Dead links are detected after about 210 s instead of about 60 s. `test_connect`
keeps keepalive disabled. The mobile transport (`mobile/ssh`) has its own
policy and is not changed. An outage longer than the budget, or an OS that
aborts the socket while a probe is unacknowledged, can still end the session.
The new log line identifies that case.

## Validation

`ssh_session::lifecycle::tests::keepalive` places a gated TCP relay between the
production client and the loopback server, and pauses the tokio clock. An idle
shell survives 150 s of silence and still answers a ping. The test fails with
the old 15 s × 3 values. A 300 s silence still ends the pane. tokio
`test-util` is enabled as a dev-only feature for the paused clock.

## Supersedes

None.

## Revisit when

A report includes the logged disconnect reason, user ssh_config `ServerAlive*`
values need to be honored, or russh changes how it counts keepalive ticks.
