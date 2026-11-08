# Delegation ownership begins at accepted input submission

## Status

Implemented locally; pending maintainer review.

## Context

Provider completion hooks identify an Agent conversation, but do not carry a
Pebrel delegation task id. Registering a delegation while an initial prompt is
still running lets that earlier turn's completion consume the new task. The
caller receives READY as success, while the actual delegated DONE is lost.

The agent_launch receipt previously exposed only ready_state from before the
initial prompt. That value proves the CLI was ready for input, not that the
initial prompt has finished.

## Evidence

The reported WSL Codex sequence returned READY after about 3.4 seconds and lost
DONE from the subsequent 90-second task. The new busy-target regression and the
registration-before-submission regression both fail against the prior local
revision c138ea1. They reproduce the defect without model calls or long sleeps.

## Decision

Reject delegation with target_busy unless the target is Idle or Finished. This
API does not queue input behind an active turn. The existing one-task-per-target
generation guard remains necessary because providers supply no shared task id.

Registration reserves correlation but is not submission. The RuntimeHub marks a
task submitted only inside the UI update that rechecks actual readiness and
conversation identity, then writes the input. Completion ignores unsubmitted
registrations. Failed input cancels the reservation. Adapters use the same hub
commit and identity rules instead of maintaining their own pending-task state.

AgentActivity retains an explicit pending-turn fact for concretely submitted
Runtime input until accepted completion, command exit or verified
conversation replacement. A delayed same-session SessionStart, screen or title
cannot make the Runtime API advertise that unfinished turn as idle. Existing
attention and failure states remain visible. Ordinary hooks alone do not establish
this override of a program's explicit OSC status. This fact is in-memory lifecycle
state, not a new task queue or persisted result cache.

agent_launch keeps ready_state for compatibility and labels its scope as
before_initial_prompt. It additionally returns the actual state_after_submission
and state_change_seq from the submission receipt, allowing a subsequent wait to
require a later state transition.

## Rejected alternatives

- Queue another prompt into a running provider: an earlier completion has no task
  id with which to distinguish the turns reliably.
- Register and immediately accept any same-session completion: the UI command may
  still be waiting behind an earlier completion event.
- Treat a stale OSC idle frame as new completion: it predates the accepted input.
- Sleep or poll until a guessed startup interval elapses: startup duration is not
  a turn-ownership boundary and repeated model prompts waste input context.
- Retry busy input automatically: the caller needs an explicit rejection rather
  than an accepted task whose execution time or ownership is ambiguous.

## Consequences

Callers must wait for the target's initial turn to finish before delegating.
Busy rejection writes no input and creates no pending callback. Existing result
formatting, output limits, immutable retries, origin identity, generation and
completion-replay guards remain authoritative. No extra model call, timer,
dependency, persistent cache or conversation rewrite is introduced.

This change does not add provider task ids or redefine mid-process-exit failure
delivery. Those require separate evidence and lifecycle handling.

## Validation

Regression tests cover initial-prompt busy rejection, unsubmitted reservations,
cancelled submissions, actual view readiness despite a stale published snapshot,
failed input cleanup, replacement identity rejection, ignored READY, exactly one
DONE, pending input surviving delayed SessionStart and stale OSC idle, and launch
receipts supplying a post-submission wait baseline. Existing WSL/native routing,
origin replacement, replay and bounded-output regressions remain required.

## Supersedes

None. Extends the completion ownership decision under
ai_hook/2026-11-08-wsl-delegation-completion.md and the bounded result contract in
2026-11-08-bounded-delegation-results.md.

## Revisit when

Providers expose a stable task or turn id in both submission and completion, or
the Runtime API gains an explicit queue contract with end-to-end acknowledgments.
