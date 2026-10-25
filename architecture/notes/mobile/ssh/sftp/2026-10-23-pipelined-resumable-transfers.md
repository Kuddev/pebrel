# Pipelined, resumable mobile SFTP

## Status
Proposed for review.

## Context
Android previously waited for each 32 KiB READ/WRITE, with another FSTAT after
every READ. Throughput was constrained by round trips even on a fast link.
Native handles and the Kotlin client were tied to a single SSH connection, so
automatic terminal reconnection could not continue a file transfer.

## Evidence
The prior loops in `SftpClient.kt` and `mobile/ssh/src/sftp.rs` serialize each
chunk. Desktop already uses 64-request windows with 32 KiB protocol chunks;
the mobile pipeline now uses the same sizing policy without changing packet
compatibility. This change does not add desktop transfer recovery.

## Decision
The native adapter aggregates up to 2 MiB per JNI call and issues at most 64
concurrent 32 KiB requests. It reassembles reads by offset, fills short reads,
and drains write acknowledgements before advancing a batch's contiguous offset.
FSTAT checks move to batch boundaries. The SSH receive window is 4 MiB.
If a batch times out, the transfer halves its window down to one protocol chunk,
so a slow link can confirm progress within the request timeout. The upload keeps
the original source buffer while acknowledging smaller contiguous slices.
Full batches confirmed within five seconds grow the window again, so a transient
outage does not permanently reduce throughput after the connection recovers.

The app keeps the SFTP client across SSH replacement. A transfer waits for the
existing session owner's recovery and acquires new native handles. Downloads
reopen the canonical path with the initial metadata revision and confirmed sink
offset. An upload creates a random token before OPEN, stages exclusively beside
the destination, and retains its unacknowledged source batch in memory. Reopening
truncates staging to the confirmed offset, then rewrites that retained batch.

Publication remains a no-overwrite SFTP v3 RENAME. If its reply is lost, recovery
checks for staging and otherwise verifies the final file's size and SHA-256
before reporting success. The already-transitive SHA-256 crate is a direct
dependency for this exceptional reconciliation path; no new resolved package
is introduced. Verification has per-request timeouts, not a total file-size
deadline. Native handles are never reused across generations.

## Rejected alternatives
Larger protocol packets risk compatibility with SFTP servers. Unbounded
concurrency risks memory and channel starvation. Resuming at remote file size
can skip holes left by out-of-order writes. Blindly repeating RENAME or deleting
the destination after a lost reply can destroy another file. A separate SSH
authentication loop would duplicate host-key, key-file and credential ownership.

## Consequences
The UI shows waiting with confirmed progress and permits cancellation. Only
network, timeout and invalidated-handle failures retry; changed files, permission
and authentication/host-key failures stop. Cancellation cleans only the owned
staging upload when connected. Offline cancellation can leave that private
temporary file on the server. No process-restart persistence is introduced:
streams and checkpoints live for the transfer task, including network recovery.
Metadata revision checking is not a cryptographic snapshot of a mutable source.
The bounded window improves latency tolerance; it is not a universal speed claim.

## Validation
Native protocol-peer tests require all 64 requests to arrive before sending
responses in reverse order, verify exact bytes in both directions, exercise
short reads and changed-source refusal, reopen ambiguous uploads at a confirmed
offset, and check final hashes after lost commit replies. Kotlin tests inject
connection replacement, lost WRITE/RENAME replies, cancellation while waiting
and permission errors. Real Android device/network measurements are separate
from these protocol and JVM tests.

## Supersedes
None.

## Revisit when
Measured links require a larger adaptive request window, or transfers must
survive process death/navigation with persisted provider permissions and state.
