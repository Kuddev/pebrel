# Native session and terminal boundaries for iOS

## Status
Implemented in the iOS foundation branch.

## Context
The mobile Rust library owns authentication, SFTP, cancellation and Noise. The
pinned VT library owns escape parsing, input encoding, graphemes and viewport
state. UIKit needs access to both without an Android runtime.

## Evidence
The [Android binding](../../../../mobile/ssh/src/bridge.rs) already dispatches
blocking operations through an integer session registry. The pinned Ghostty API
provides a reusable row iterator, per-row dirtiness and caller-owned UTF-8 output.

## Decision
Keep blocking SSH and Link calls in the Rust library. Provide an explicit C
header with owned returned strings, bounded byte spans and separate error and
result channels. Share the same handle registries across foreign bindings.
Use per-SDK static archives so the arm64 simulator receives simulator objects.
Serialize VT access on its owner and export one viewport row at a time.

## Rejected alternatives
A generated cross-language object model would introduce a generator and runtime
for two small interfaces. Reimplementing host verification or Noise in Swift
would require independent protocol and cancellation tests for the same behavior.

## Consequences
Foreign callers free returned strings, discard revoked IDs, wipe their own
credential storage and run blocking network calls off the main thread. A renderer
can retain unchanged rows and retry an oversized grapheme without reparsing input.

## Validation
The C client exercises first trust, authentication, opening a PTY, byte streams,
exit and revoked handles against an ephemeral loopback OpenSSH server. Simulator
builds and archives select different SDK slices.

## Supersedes
None.

## Revisit when
A new consumer needs a different concurrency or ownership model, or upstream
offers compatible row and input APIs directly to Swift and Kotlin.
