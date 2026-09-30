# Discover persistent sessions without writing to the terminal

## Status
Accepted for implementation.

## Context
Saved SSH hosts can launch a named persistent session, but users cannot browse
already running sessions and select a window from the mobile terminal.

## Evidence
The existing SSH transport supports PTY exec and independent SFTP channels.
The session managers expose read-only session/window queries. The workspace/tab
CLI returns JSON directly; it does not accept the old draft's extra `--json` flag.

## Decision
Reuse the authenticated SSH connection for a separate exec query channel, with
one queued request, a ten-second caller deadline and a 64 KiB combined stdout/
stderr budget. Channel opening and command results each have a four-second
budget, followed by bounded cleanup. Terminal and SFTP futures remain independent.
Keep discovery under the local session owner and reject results after it closes.
Closing a picker cancels its coroutine; a blocking JNI call can finish only at
its native deadline or when the session closes.

Use a POSIX shell explicitly on Unix hosts, including common macOS executable
locations. Windows OpenSSH uses an encoded PowerShell script and discovers its
native persistent sessions too. The selected host syntax travels with the result.
Opening a listed session creates a separate PTY connection through the existing
credential/trust flow. Use session IDs and creation/server identity for Unix sessions;
never interpolate a displayed session title as executable syntax.
Persist normalized host OS icons as optional metadata without changing computer
identity, endpoint discovery, authorization or legacy invitation parsing.

## Rejected alternatives
Injecting list commands into the interactive shell contaminates terminal output.
Running queries in the PTY loop blocks keyboard processing. Copying the old
transport would remove newer SFTP support. Treating every host as POSIX excludes
Windows OpenSSH. Selecting by a mutable session name can attach to another target.

## Consequences
Only managers installed and accessible to the authenticated SSH user appear.
No manager is installed and no session is created by discovery. A new connection
may require the existing login/trust interaction. Closed or recreated targets can
fail attachment; failures remain visible through the ordinary terminal status.
This is discovery of existing sessions, not persistence of SSH credentials in a
second store or restoration of an already disconnected PTY.

## Validation
Existing Android tests cover parsing Linux/macOS/Windows responses, stable target
selection, command quoting and saved profile compatibility. Native SSH tests
cover cancellation and bounded input. Real remote-manager and device interaction
remain separate acceptance work.

## Supersedes
None.

## Revisit when
The managers expose a negotiated protocol independent of shell command execution.
