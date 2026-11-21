# Verify the asset terminal before using an SSH startup command

## Status

Proposed for the login failure reported in #1078.

## Context

A gateway can acknowledge a client PTY while forwarding the following exec
command to an asset over pipes. The POSIX quoting probe succeeds over those
pipes. Pebrel can then start a login shell without an interactive terminal.

## Evidence

KoKo v4.10.19 enters interactive dispatch when a session has a PTY and an empty
raw command. Its command handler requests an asset PTY only for particular bash
login commands. Pebrel's wrapped startup command does not have that form.
[KoKo session and command dispatch](https://github.com/jumpserver/koko/blob/v4.10.19/pkg/handler/server_ssh.go).

## Decision

Open a temporary PTY with the window size and terminal type used for the shell.
The capability probe checks that both standard input and output are terminals
at the process executing the command. It then verifies the existing nested
quoting contract. A single three-second budget covers the request and reply.
When the check fails, use the ordinary PTY and SSH shell request.

## Rejected alternatives

Hiding a missing-tty marker fixes its display but does not give the asset a
terminal. Matching a software banner would hard-code a gateway product into
the SSH adapter and would miss other gateways with the same forwarding behavior.

## Consequences

The capability probe now requests a PTY. Normal POSIX servers retain integrated
startup and remote multiplexer recovery. A gateway that runs exec commands
through pipes gets its interactive shell entry point.

## Validation

The startup test runs the probe with pipes and a real PTY. The loopback test
checks that the probe uses its requested PTY, reports that the asset terminal
check failed, and sends a welcome message through the ordinary SSH shell.
[Startup tests](../../../../pebrel_app/src/ssh_session/resume/tests/startup.rs)
and [channel tests](../../../../pebrel_app/src/ssh_session/lifecycle/tests.rs).

The [OpenSSH acceptance test](../../../../pebrel_app/src/ssh_session/resume/acceptance.rs)
passed against an isolated sshd on Linux. Two panes shared one transport.
Reopening one restored its saved tmux window and pane, and the other pane
kept its target.

## Supersedes

None. This extends the quoting check described in the
[POSIX startup note](2026-11-01-posix-startup-quoting.md).

## Revisit when

A gateway grants a working PTY to the probe command but changes the descriptor
types for the actual integrated shell command.
