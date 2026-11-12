# Detect stale SSH transports when Android resumes

## Status
Accepted for implementation.

## Context
Android can suspend a connection without changing the reported default network.
An established terminal may still appear ready when the activity returns even
though its TCP socket no longer works. Route-change recovery alone misses this.

## Evidence
`foregroundChanged` checked desktop relay liveness but did not check direct SSH.
`SshConnection` reports a missing native handle as `CLOSED`; that code previously
fell through to an unknown failure, outside the transient retry policy.

## Decision
On foreground entry, probe established ready SSH tabs over their authenticated
query channel. A short `exit` command does not write to the interactive PTY.
Await the blocking JNI operation from a separate coroutine with a five-second
deadline. On timeout or a transient transport error, close the terminal transport
and request recovery inside the existing tab. Native session cancellation shuts
down the socket and releases waiting queries.

Apply the result only while foregrounded and while the original connection and
terminal still own a ready tab. A channel refusal or authentication/trust error
does not restart an otherwise healthy terminal. Backgrounding or closing the tab
cancels its pending probe; native queries also have an independent timeout.

## Rejected alternatives
Waiting only for a network-change callback misses stale sockets on the same route.
Putting the probe text into the PTY could send a command to remote multiplexer or another running
program. Running blocking JNI inside the deadline coroutine would delay timeout
handling until the native operation returned.

## Consequences
Foreground entry adds one bounded query per established SSH tab. This detects
transport loss, not Android process death. Recovery retains the existing remote multiplexer
attachment policy and does not invent a target for an opaque shell.

## Validation
`SshRecoveryTest` covers recovery from a closed handle without replacing the tab
identity, in addition to background route recovery and stale callback rejection.
`SshStatusTest` covers the native error classification. The focused 35-test JVM
suite and architecture checks passed. Native close/cancellation was inspected;
phone sleep/resume and APK acceptance have not been run on this host.

## Supersedes
None. Extends the recovery ownership in `2026-10-22-inline-ssh-recovery.md`.

## Revisit when
The native SSH layer exposes a dedicated bounded liveness request or recovery
needs to survive Android process termination.
