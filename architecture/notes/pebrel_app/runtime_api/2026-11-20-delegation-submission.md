# Correlating a completion with a submitted delegation

## Status

Implemented.

## Context

The runtime receives agent.delegate on a transport thread and writes its prompt on the UI thread. Provider hooks can identify a session but carry no Pebrel task ID.

## Evidence

A completion from an earlier turn could arrive while the new delegated prompt was queued. The first matching completion removed the only pending task for that worker generation. The caller received the previous answer and missed the delegated answer.

## Decision

Reserve the worker generation after checking its published state. Immediately before writing, both desktop adapters check the live worker state and the provider identity captured by the reservation. The UI records the next local hook receive sequence. Only a completion received at or after that point may consume the task. A failed write releases the reservation. A request timeout cancels a queued write while preserving an already submitted task.

## Rejected alternatives

A check confined to the transport thread leaves the UI queue race open. A provider timestamp can use a different clock. The parser already assigns each hook a monotonic local sequence.

## Consequences

A busy worker returns target_busy. The caller can wait for the worker to finish and submit again. Existing direct agent prompts retain their input behavior.

## Validation

The submission tests exercise late delivery of an earlier hook, an already busy worker, a worker that changes after registration, cancellation on either side of the write, and a failed write followed by another reservation.

## Supersedes

None.

## Revisit when

The provider supplies a task identifier shared by the input submission and its completion.
