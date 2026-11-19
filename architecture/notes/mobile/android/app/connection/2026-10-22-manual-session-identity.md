# Track remote multiplexer entered from an ordinary SSH shell

## Status
Accepted for implementation.

## Context
The explicit remote multiplexer attachment path can announce its PTY before attaching. A normal
SSH shell must retain its own startup behavior, but the user may subsequently run
remote multiplexer or a shell command that attaches it. Restricting history/recovery control to
configured remote multiplexer sessions left that common path without a saved remote target.

## Evidence
An isolated OpenSSH server with two independent connections showed that an exec
channel and its connection's PTY processes share the nearest per-login sshd
ancestor. The other connection has a different ancestor and terminal device.

The same probe exposed a separate prerequisite: without a locale in the exec
channel, remote multiplexer 3.4 replaces tab separators and non-ASCII metadata. Unit fixtures
containing tab-separated output had not exercised this producer behavior.

## Decision
After successful POSIX discovery, read a bounded process snapshot through the
existing authenticated query channel. Associate its shell PID with the nearest
known OpenSSH process, and accept only one distinct terminal device belonging to
that ancestor. Cache that device for this connection. Unknown servers, missing
parents, cycles, oversized output and multiple terminal devices yield no match.

Reuse the existing remote multiplexer controller to match that device to a client, capture its
session/window/pane identity, and route history commands. An intentional detach
clears the automatic target and restores ordinary shell/application input. Query
failures retain the last known target. Metadata listing explicitly requests remote multiplexer
UTF-8 output with `-u`; the user's shell locale is unchanged.

## Rejected alternatives
Choosing the first or only named remote multiplexer session can attach another task. Inferring
ownership from a title or a stale inherited `SSH_CONNECTION` value is insufficient.
Replacing the login shell with a custom bootstrap changes startup and platform
behavior. Injecting identity commands into the interactive PTY can execute them
inside a running application instead of the shell.

## Consequences
Manual remote multiplexer works for an identifiable POSIX OpenSSH login and the default remote multiplexer
server. Other SSH implementations, ambiguous connections and inaccessible process
metadata retain normal terminal input; there is no guessed reattachment. The
initial process query runs off the UI thread and is bounded by native query
limits. Existing checkpoint timing still applies. An observed target is restored
with the same non-creating attachment command as an explicitly selected session.

## Validation
`SshPtyIdentityTest` covers connection separation, ambiguity, ancestry cycles and
known OpenSSH/Darwin naming. `TmuxSessionControlTest` covers manual attachment,
history/input control, detach and fallback. `TmuxDiscoveryTest` runs an isolated
real remote multiplexer server without a locale and checks separators plus Unicode names; it
reports a skip on hosts without remote multiplexer instead of pretending to verify them.
The compiled production controller and attachment commands also passed an
isolated two-connection OpenSSH/remote multiplexer probe: manual attachment, cursor-preserving
scroll, input restoration, window changes, detach, and reconnection to the saved
window/pane while the other connection remained alive.
Phone lifecycle and APK acceptance remain separate from JVM and server probes.

## Supersedes
Extends `2026-10-22-remote-history-ownership.md` to ordinary SSH shells; the existing
startup marker remains authoritative for explicitly launched remote multiplexer attachments.

## Revisit when
Supporting a server with different process ownership, multiple PTYs per native
SSH transport, custom remote multiplexer sockets, or server-assisted identity notifications.
