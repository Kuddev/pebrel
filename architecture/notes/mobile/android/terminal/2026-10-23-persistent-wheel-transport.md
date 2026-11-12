# remote multiplexer wheel transport

## Status
Proposed for review.

## Context
The 2.2.1 application mouse path writes negotiated VT reports to the existing PTY.
The later history ownership change intercepted every fullscreen wheel gesture,
querying the remote multiplexer client and executing a second command before accepting the next
gesture. Network round trips filled the bounded input queue even without transfers.

## Evidence
`c6ef7618` introduced unconditional remote history interception. Desktop wheel
routing did not acquire this interception; this finding does not establish the
cause of the user's Windows latency report.

## Decision
Recheck active screen and mouse tracking at dispatch. Mouse reporting uses the
native encoder and PTY directly. Without mouse reporting, retain guarded remote multiplexer
history operations so scrolling cannot type arrow keys into shell history.
Adjacent same-direction gestures for the same coordinates and input generation
coalesce while IO is busy. Each batch is bounded to 256 lines / 32 KiB of reserved
encoding space. Text, keys, replies and direction changes preserve queue order.
The next input checks copy mode once; it does not query remote multiplexer on every gesture.

## Rejected alternatives
Raising queue limits alone preserves the round-trip bottleneck. Removing remote
history control entirely reintroduces shell-history arrows and typing into copy
mode. Enabling remote multiplexer mouse globally would mutate the user's server configuration.

## Consequences
Mouse gestures no longer depend on the independent exec channel. The fallback
still needs remote execution, but bursts share requests. Real disconnection and
input failures remain visible. Native VT state, rather than a stale UI frame,
decides whether queued wheel input may still target the application.

## Validation
Robolectric `TerminalScrollTest` and `TmuxSessionControlTest` pass: PTY mouse input
survives failing history queries; 500 queued fallback gestures preserve distance
with two pending batches; primary-screen transitions do not type wheel input;
copy mode is left before subsequent text. JNI mode assertions are included in
`GhosttyEngineTest`; this local JVM run does not execute Android JNI.

## Supersedes
The unconditional exec routing in
[remote history ownership](../app/connection/2026-10-22-remote-history-ownership.md).

## Revisit when
The native engine exposes a different mouse negotiation contract, or measured
fallback latency warrants a dedicated remote history protocol.
