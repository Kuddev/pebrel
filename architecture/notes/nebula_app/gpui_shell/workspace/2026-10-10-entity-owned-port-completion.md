# Entity-owned SSH Ports completion

## Status

Proposed for review. Actual check outcomes belong in the PR.

## Context

A pending Ports operation can outlive a tab transfer. Its terminal entity survives
in the destination window, while the source window can close.

## Evidence

The original callback captures the source WindowHandle. If updating it fails,
the surviving terminal retains Some(finished_task) and another Ports operation
remains disabled. GPUI maps an entity to the most recently rendered window, which
also leaves a gap before the destination's first draw.

## Decision

Store the task on the terminal and complete through its live entity. Clear pending
state even when the original window has closed. Check the expected SSH destination
before installing a result; use the current rendered host only for its toast.

## Rejected alternatives

- Keeping a source window alive just for a task couples transport and window lifetime.
- Resolving only the last rendered window still fails before the destination draws.
- Clearing pending only on success leaves failed operations permanently disabled.

## Consequences

Transport completion is independent of window existence. A toast may be omitted
while no rendered host is available; task state still completes and stale results
are dropped. Existing local forwarding policy and persistence remain unchanged.

## Validation

The real task/completion helper is exercised with a gated synthetic result. The destination root
retains the terminal without rendering it yet (the test API draws newly opened
windows immediately). Close the source window, verify its handle is unusable and
the terminal has no current rendered host, then deliver refusal. Assert pending
state clears before the terminal renders in the destination and a subsequent
operation completes after rendering. This does not prove
native tab dragging or authentication against a production SSH server.

## Supersedes

The captured-source-window completion in the existing local Ports dialog.

## Revisit when

Port operations acquire persistence, reconnect behavior or a distinct task identity.
