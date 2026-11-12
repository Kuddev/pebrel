# Restore the remote target owned by each SSH pane

## Status

Accepted for implementation.

## Context

Retry previously created a fresh shell with the previous cwd. A pane that entered
a remote multiplexer manually consequently lost its attachment. Desktop panes may
share an authenticated transport, so identifying the only PTY under an SSH server
process is insufficient and can select another pane.

## Evidence

`workspace/ssh_reconnect.rs` previously preserved only destination and cwd. The
isolated OpenSSH acceptance now opens two PTYs on one authenticated connection,
enters a private named-socket server, changes a window, replaces one channel and
verifies that the saved target and unaffected peer remain distinct.

## Decision

The SSH adapter owns a typed, memory-only recovery target with executable, socket,
session ID, creation time/server PID, window and pane. The view retains a snapshot
only for retry of that same pane. Duplicate tabs and application session restore
continue to open fresh shells; they do not inherit another pane's attachment.

A nonce-scoped startup marker identifies the exact PTY and shell PID even with a
pooled transport. Its bounded filter preserves banners, malformed/foreign bytes
and truncated markers, then passes subsequent bytes through without allocation.
Supported POSIX shells use a startup exec wrapper; restricted/non-POSIX servers
retain the existing ordinary-shell fallback.

A cancellable asynchronous monitor uses separate exec channels. Queries inspect
only process names/PIDs/TTYs and server executable/socket metadata, not arguments,
environments or pane contents. Queries and each full checkpoint have three-second
budgets; polling waits two seconds after completion. No probing occurs in input,
scrolling or rendering callbacks. The monitor is dropped when its pane pump ends.

Only a still-owned shell without a client clears an attachment. Lost ownership,
timeouts and transport errors preserve the last confirmed target. The replacement
view gets its own state, so delayed results cannot overwrite the new connection.

Retry validates session identity, then selects the original window and pane. A
missing/reused target or failed attachment enters the default shell. It does not
create another session. Remembered cwd is applied inside the startup wrapper only
when starting the shell, never injected into an attached application.

## Rejected alternatives

- Host-keyed state or selecting the first session: multiple independent tasks use
  the same authenticated host connection.
- Replaying the launch command: aliases and wrappers conceal actual endpoints;
  commands can have side effects and contain sensitive arguments.
- A lost-client result as proof of detach: transport loss has the same symptom.
- Synchronous queries in the input/output pump: network delay would regress typing.

## Consequences

Windows, macOS and Linux use the same desktop transport implementation. Android
retains its own adapter with equivalent endpoint/identity/fallback semantics.
Recovery requires supported remote metadata access. Linux discovery uses `/proc`;
macOS uses `lsof`. No new dependency, on-disk history or remote helper is installed
for recovery. Plain shell jobs cannot survive loss of their PTY without a remote
session manager.

## Validation

94 SSH regression tests pass (two opt-in tests excluded), including packet
boundaries, malformed identity, pooled PTY isolation, failed attach, hook bootstrap,
cancellation, backpressure and literal paths. The opt-in isolated OpenSSH acceptance
passes against a private named socket and separate executable: exact window/pane
restoration, intentional detach, missing-target shell fallback and unaffected peer.
Remote macOS discovery and physical Windows/macOS UI acceptance remain unverified.

## Supersedes

None; extends pane-scoped transport ownership with retry metadata.

## Revisit when

SSH offers a portable authenticated channel-to-PTY identity mechanism, or recovery
after whole-application process death becomes part of the persistence contract.
