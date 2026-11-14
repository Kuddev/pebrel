# Typed SSH tab identity follows the connection context

## Status

Proposed in PR #841.

## Context

A manually entered SSH command runs inside a local pane. Remote shell integration
can emit OSC 133 command-end markers while the local SSH client remains connected.
The foreground program and the enclosing connection have different lifetimes.

## Evidence

The original tab label disappeared on the first remote OSC 133;D. A loopback
OpenSSH PTY reproduced this with the local client still alive. The regression in
`gpui_shell/terminal/view/startup_tests.rs` replays the captured output through
the terminal parser and view event path, including the subsequent disconnect.

`CompletionContext` already restores enclosing environments only when their
recorded shell identity is reported again. The local connection wrappers emit
that identity after the SSH invocation returns. Remote command ends do not pop
the existing context stack.

## Decision

Keep the parsed SSH destination on the existing connection frame. The tab adapts
the outer typed connection into a cached saved-host label. Foreground command
cleanup does not remove this connection label; restoring the enclosing shell
frame removes it. Built-in SSH panes retain their existing host identity, and
native AI task titles keep their precedence.

Cache the display label by destination, so ordinary shell/user-variable events
do not reread the profile file. Parse argv through the existing SSH authority;
keep an IPv6 address bracketed when adding a nondefault port.

## Rejected alternatives

- Treating every OSC 133;D as disconnect conflates remote command and local
  connection lifetimes.
- Maintaining another shell-token stack in the view duplicates the established
  protocol owner and its alias/argv reconciliation.
- Polling processes from tab rendering blocks the UI and cannot reliably inspect
  commands running inside WSL or on another host.

## Consequences

Nested remote commands and shells retain the outer connection's host label.
Alias/variable-expanded argv can correct its provisional destination. Restoration
uses the existing shell-identity contract; a bare command-end or directory report
is insufficient evidence of disconnect. Shells that never report a known parent
identity have the same restoration limitation as completion history.

## Validation

Regressions cover remote command completion followed by the parent shell return,
captured OpenSSH output, nested contexts, expanded connection arguments, and two
saved profiles for the same IPv6 address on different ports.

## Supersedes

None.

## Revisit when

The shared connection context gains a transport/process-exit signal, or the shell
identity protocol changes. Extend that authority rather than adding a view-owned
connection state machine.
