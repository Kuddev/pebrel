# Completing a delegation from the pane that accepted the turn

## Status

Implemented.

## Context

Native hooks reached the global workspace dispatcher. WSL and SSH hooks reached the terminal view in an authenticated OSC envelope. Both could update activity, but only the global dispatcher notified the runtime that a delegated worker had finished.

## Evidence

The OSC ingress discarded the receiving pane ID during decoding. A worker could show a completed turn while its caller waited indefinitely. The native dispatcher also treated an ignored lifecycle event as routed, which could pass a foreign process's completion to the runtime.

## Decision

The OSC decoder assigns the local receiving pane. After lifecycle ownership, ordering and recovery checks succeed, the terminal view emits a completion only when the event identifies that pane. A native event with no specified pane can still update the focused view. The workspace defers callback processing until it can publish current pane state and safely re-enter the originating window.

## Rejected alternatives

Calling the runtime directly from the OSC decoder would bypass the pane's lifecycle and ordering checks. Using the pane ID written into the OSC envelope would let the producer choose another pane. Polling a screen for the final answer loses both the provider turn boundary and its identity.

## Consequences

Both ingress paths use the same worker, session and generation checks. A busy caller keeps its callback queued until it is ready. Terminals with no pending delegation skip the extra workspace snapshot.

## Validation

Workspace regressions pass OSC events through the terminal event entry point and inspect the input sent to the caller. The cases cover both provider directions, a busy caller, a replay between tasks, another process in the same pane, and addressed and unaddressed native events.

## Supersedes

None.

## Revisit when

The provider supplies an authenticated task identifier on its submission and completion paths.
