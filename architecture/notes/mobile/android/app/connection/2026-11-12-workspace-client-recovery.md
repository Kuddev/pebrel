# Attach only to the selected remote workspace

## Status

Proposed.

## Context

An Android SSH tab can launch a workspace on its first connection or open a
workspace from the server's running-session list. When an existing target ends,
opening a fresh shell under the same name loses the context that reconnect was
intended to restore.

## Evidence

In a Linux PTY, the former reconnect command kept the original shell PID while
the daemon was running. After the daemon stopped, the same command started a
new daemon and a shell with a different PID. On 0.9.5, the upstream client mode
alone also tried another saved SSH machine when its local target was absent.

## Decision

Use the upstream `client` entry point for recovery and for an explicit selection
from the running-session list. Set `HERDR_REMOTE_KEYBINDINGS=server` for that
process to use the selected host's keymap and skip its saved-machine catalog.
An unnamed host profile keeps the same remote default as its first connection.
A selected tab must still be focusable before the client is started.

## Rejected alternatives

- Check whether the server is running before the launch-or-attach command. It
  could stop between the two operations and the launch command would create it.
- Attach to a single terminal stream. That would drop the workspace and tab
  controls and is unavailable on Windows.

## Consequences

First connections can still create a workspace. Missing targets terminate the
PTY command and leave the SSH tab in its existing ended state. A server's
saved-machine federation is available when launched manually from a regular
SSH shell. Client-only mode depends on an upstream internal entry point.

## Validation

The POSIX regression runs the production commands against a fixture executable.
Real Linux PTYs with upstream 0.8.2 and 0.9.5 verify shell continuity and missing
targets. The 0.9.5 fixture also verifies a saved-machine probe is suppressed and
a custom server shortcut still works.

## Supersedes

None.

## Revisit when

Upstream offers a documented attach-only command for a named workspace.
