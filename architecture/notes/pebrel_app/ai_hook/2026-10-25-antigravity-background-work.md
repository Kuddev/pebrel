# Antigravity Stop is not completion while background work remains

## Status

Implemented for PR #518 review.

## Context

The initial integration classified `model_stop` as Done even when `fullyIdle` was
false, emitting completion while asynchronous work continued.

## Evidence

The [official Stop schema](https://www.antigravity.google/docs/hooks/#stop), checked
on 2026-10-25, defines `fullyIdle` as required: false means background commands or
asynchronous tasks remain active. Stop output requires a decision; `continue`
restarts the execution loop and any other value allows stopping.

## Decision

Normalize false to the existing background-work presence contract (one means at
least one, not an exact count). True clears that presence. The shared lifecycle,
ordering and notification rules already defer completion for background work.
Missing/invalid idle evidence cannot establish success. Preserve failure outcomes.
The helper returns `{"decision":"stop"}` for Stop and `{}` for other subscribed
events, including when the notification transport is unavailable.

## Rejected alternatives

- Ignoring the field: premature Done and completion notification.
- A separate Antigravity lifecycle: duplicates the shared state authority.
- Returning `continue`: observing the CLI must not cause it to do more work.

## Consequences

No dependency or persisted state changes. The normalized count is a lower bound.
A later fully-idle Stop can establish completion; no timer invents that event.

## Validation

The real parser/lifecycle regression checks false -> Working and true -> Done.
The application hook suite passed 152 tests with one existing ignored test.
A real helper-process regression checks the required Stop stdout decision.
No live Antigravity CLI session was exercised.

## Supersedes

The `fullyIdle` and empty Stop output assumptions in
[the original integration note](2026-10-07-antigravity-hooks.md).

## Revisit when

The provider documents exact task counts or changes Stop delivery semantics.
