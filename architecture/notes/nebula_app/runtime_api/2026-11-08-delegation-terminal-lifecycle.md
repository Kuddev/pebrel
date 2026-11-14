# Terminal lifecycle facts finish outstanding delegations

## Status

Implemented locally; pending maintainer review.

## Context

Provider completion hooks finish normal delegations, but a process or pane may
exit before it can send a final hook. The managed Agent registry already records
these facts and wakes agent.wait, while pending delegations previously remained
unresolved. An original caller could wait indefinitely without a failed result.

An agent.wait receipt also required callers to locate its target inside a full
snapshot to obtain the matched state and sequence. Reading those fields directly
from result returned null despite a successful wait.

## Evidence

Against local revision 26bced6, the exit regression receives zero callbacks and
the direct wait-state regression receives null. Neither regression invokes a
model or uses a long sleep. Normal DONE was independently observed in the real
Codex route and remains a required positive contract.

## Decision

RuntimeHub reconciles its bounded pending delegations when it publishes canonical
registry/pane lifecycle facts or explicitly closes an Agent. An accepted task
whose exact target generation is closed is removed and converted into one failed
callback. A stable reason identifies pane_closed, pane_exited, agent_exited,
agent_replaced or agent_closed. The first pane lifecycle cause remains authoritative.

This is a Runtime failure result, not an invented provider event. It contains no
provider answer: output_missing=true and worker_output=null. Formatting reuses
the normal callback header and queue. Elapsed time and the full callback are
frozen once, so deferred delivery and retries preserve the original result.

Reconciliation releases an unsubmitted registration without notifying a task
that was never accepted. A lost/replaced original caller also releases pending
ownership and cannot redirect a result to the focused pane or another generation.
Delivery validates both the current pane identity and any managed registry owner.

The first accepted terminal outcome owns the task. A normal result already queued
before an exit stays the result. If exit wins, a late completion cannot generate
a second callback. A busy original caller keeps the same bounded delivery queue.

agent.wait adds task_state and state_change_seq beside its existing agent and
snapshot. Both scalars come from the exact pane that satisfied the original
generation, state and after_seq checks; no new wait implementation is introduced.

## Rejected alternatives

- Depend on every provider sending Stop during process teardown: the failure is
  specifically that such a hook may never exist.
- Forge a provider TurnDone to reuse formatting: lifecycle failure must not
  fabricate an answer, outcome fact or turn identity.
- Use a terminal tail or the previous answer as a failed task's output: it can
  belong to a different task and would hide missing data.
- Leave pending ownership until a client timeout: a wait timeout does not cancel
  a delegated task, and abandoned entries can consume capacity indefinitely.
- Add polling timers, model summarization or another persistent task store: the
  existing canonical lifecycle and callback retry loop already own these facts.
- Replace the complete wait snapshot with only the convenient scalars: existing
  callers still need its canonical layout and identity projection.

## Consequences

No extra model call, history rewrite, dependency, answer cache or task timer.
The existing bounded pending/callback capacity and original-session guards apply.
Intentional pane closure is reported as failed with a precise closure reason;
it is not inferred to be a provider-confirmed cancellation. This does not create
a task deadline or automatically kill a worker when its original caller leaves.

## Validation

Regressions cover process/pane exit, explicit registry closure, replacement,
unsubmitted and origin-lost cleanup, pending-capacity release, both completion/exit
orders, busy-origin deferral, retry immutability, late hooks and wait receipt
compatibility with after_seq. GPUI tests exercise terminal exit and the resulting
automatic callback input. Existing WSL/native, submission and output tests remain.

## Supersedes

None. Extends 2026-11-08-delegation-submission-ownership.md and the bounded result
contract; it closes the separately recorded mid-process-exit delivery gap.

## Revisit when

Providers supply reliable end-to-end task ids or Runtime gains a separately
defined durable task/deadline service across application restarts.
