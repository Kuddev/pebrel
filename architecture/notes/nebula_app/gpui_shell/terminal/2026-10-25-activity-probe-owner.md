# Reject process evidence from before a new hook owner

## Status

Implemented during PR #675 review.

## Context

Moving process enumeration off the UI thread creates a window where a hook can
identify an Agent before an older process snapshot arrives. Command start time and
shell PID alone do not change when that happens.

## Evidence

The GPUI regression starts with a probe lacking a primary PID, establishes a live
Claude hook owner, then applies the old empty snapshot. Without an owner check the
result clears the running Agent as if its process had exited.

## Decision

Capture the primary Agent PID with the command/shell snapshot and require all
three to still match before applying evidence. A failed probe remains no evidence.

## Rejected alternatives

- Command/shell identity alone: misses newly established hook ownership.
- Ignore all empty snapshots: loses valid process-exit reconciliation.

## Consequences

No new persistent state or timer. A stale batch is discarded; the next watchdog
sample observes the current owner.

## Validation

64 related tests passed, including the real GPUI stale-owner regression and its
positive control showing that current-owner exit evidence still clears state.
Native CI for the updated PR remains separate.

## Supersedes

Completes the asynchronous probe ownership rule in
[the idle-cost note](2026-10-12-idle-render-frame-budgets.md).

## Revisit when

Process identity moves from PIDs to a generation-bearing platform handle.
