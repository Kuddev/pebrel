# Explicit client states retain lifecycle authority

## Status

Accepted following the maintainer's request to map client states directly.

## Context

The remote adapter could replace a Hook-owned state after twenty seconds of
silence and two matching screen samples. This allowed a client-reported wait or
completion to become working, or working to become completed, without another
client event. The maintainer requires running to map to the shared spinner and
permission/question waits to the shared hand, without these inferences.

## Evidence

`AiHookCapabilities` already distinguishes full lifecycle, turn-only and
completion-only integrations. The shared reducer's ordinary screen path
respects that distinction; the remote reconciliation path bypassed it.

OpenCode's current official plugin reference lists `permission.asked`,
`permission.replied`, `session.status` and `session.error`. The bridge handled
older permission names and the named callback, but omitted the newer event-bus
names. Its V2 adapter also reduced success, failure and interruption to one idle
event, losing the client's explicit outcome.

## Decision

Remove the timed remote lifecycle override and its inferred completion state.
Complete Hook integrations retain state until another accepted client signal,
an explicit user submission, or a real command/session boundary. OSC 7501
records and verified native title signals keep their existing direct mapping.
Screen compatibility fills only the gaps allowed by the installed bridge's
capabilities; it cannot rewrite a complete lifecycle because time passed.

Normalize OpenCode's current permission/question events to attention and reply
events to resumed work. Carry V2 execution outcomes through the bridge unchanged
in meaning. A V1 idle event without an outcome returns to idle rather than
claiming successful completion. The UI uses its existing spinner/hand mapping.

## Rejected alternatives

- Shorter timers or larger keyword lists: still infer a state the client did
  not report.
- One symbol table applied to every CLI: providers do not share a title or Hook
  contract. Identity and source-specific adapters remain necessary.
- Treating every idle callback as success: cancellation and errors can also
  end execution.

## Consequences

A lost event is no longer repaired by guessing from screen text. The transport
and integration must deliver the event, or another explicit source must report
the current state. Older clients with incomplete APIs retain only their
documented partial coverage. Direct reporting describes the received client
state; it does not prove that an unobserved remote event never occurred.

## Validation

Existing authority scenarios were updated for the intentional contract change.
No regression suite is run at the user's request. Format, architecture checks
and the real candidate product build provide the executed verification.
Provider references and binary research remain outside the repository.

## Supersedes

[Timed remote turn reconciliation](2026-10-16-remote-turn-reignite-reconciliation.md).

## Revisit when

A provider adds structured current-state queries or changes its event schema.
