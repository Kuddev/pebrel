# Revoke screen attention when its form evidence disappears

## Status

Proposed for review.

## Context

[#662](https://github.com/Kuddev/pebrel/issues/662) reports a running Claude Code
turn remaining in attention across many screen updates. Runtime identifies the
state as `screen` / `live_form_blocked`, without a lifecycle hook. Later snapshots
contain none of the form's trigger phrases, but the attention state persists.

The [existing authority decision](2026-09-19-agent-activity-authority.md) preserves
stronger hook and process facts when screen classification has no match. Applying
that retention to a screen observation itself gives a disappeared form unlimited
authority. Both UI shells project the same shared state, so its lifetime belongs
in the shared owner.

## Evidence

- [`AgentActivity::observe_screen`](../../../../nebula_app/src/ai_hook/lifecycle.rs)
  accepts screen Blocked evidence. Before this repair, its no-match path only
  reset consecutive idle samples. Without a later matching rule, that screen
  state had no path out.
- [`detect`](../../../../nebula_app/src/ai_agents.rs) deliberately returns `None`
  for an unclassified snapshot. It does not establish idle or completion.
- The [GPUI adapter](../../../../nebula_app/src/gpui_shell/terminal/view/agent_activity.rs)
  and [legacy adapter](../../../../nebula_app/src/window_context/agent_activity.rs)
  also reset samples when an agent cannot be identified. That absence is distinct
  from a real snapshot of a known agent whose form no longer matches.

## Decision

Keep the evidence lifetime in `AgentActivity`. Only two consecutive actual
snapshots of a known agent with no matching rule revoke a Screen-sourced Blocked
state. A matching observation interrupts that sequence; a persistent form remains
Blocked. This count tolerates one unclassified redraw without introducing a timer.

On entering screen Blocked, remember the preceding Working state's source and
rule, when one exists. Repeated Blocked observations preserve that fallback. When
the form evidence expires, restore that Working state and its original metadata;
otherwise return to Unknown with no claimed source or rule. Preserve the existing
turn and submission semantics. Revoking a form does not establish completion.

Accepted hooks and explicit submissions discard fallback evidence from the prior
state. Explicit input resets consecutive sample counters. Command reset and the
next command clear all of this state. An unidentified agent uses a separate
sample-reset operation that preserves status, rather than contributing a no-match
snapshot. Both UI adapters retain their existing projection and notification path
for real shared-state changes.

Hook-sourced attention keeps its existing authority. Full lifecycle hooks continue
to bypass screen classification; partial bridges retain the same capability limits.

## Rejected alternatives

- Retain every screen Blocked indefinitely: reproduces the reported latch after
  its only supporting evidence has disappeared.
- Clear attention after elapsed time or infer Done from no output: neither proves
  that a form disappeared or that a turn completed.
- Turn every unclassified frame into Idle: can create false completion edges and
  discard stronger hook or process evidence.
- Broaden spinner expressions, increase scanned regions, or repair hook identity
  in the same change: those are separate detection and routing questions requiring
  their own captured evidence; they do not bound this state lifetime.
- Add independent view-level expiry rules: forks lifecycle authority between UI
  shells and leaves Runtime and notifications with inconsistent state.

## Consequences

The shared owner gains bounded in-memory fallback metadata and a consecutive
sample counter. No dependency, persistence format, worker or timer is introduced.
Expiry produces neither an idle classification nor a completion notification.
Returning to Working preserves the existing evidence, rather than claiming the
unclassified frame proved new work. An initially unclassified state stays Unknown
until a later rule, hook or process fact decides it.

## Validation

Production regressions in the existing
[lifecycle tests](../../../../nebula_app/src/ai_hook/lifecycle/tests.rs) cover source
and rule restoration, Unknown fallback, interrupted sample sequences, persistent
forms, input and command boundaries, partial hooks, full hook authority, and the
absence of completion notifications. The existing
[GPUI activity test](../../../../nebula_app/src/gpui_shell/terminal/view/activity_tests.rs)
exercises actual terminal snapshots and badge projection through the product
adapter. Reinstating the original no-match path makes both Claude regressions
fail because Blocked persists after the form disappears. Both pass with this
repair on Linux using the repository CI profile. Full validation results and
platform limits belong in the accompanying PR evidence; no live-provider or
visual acceptance is claimed here.

## Supersedes

None. Extends the evidence lifetime boundary of
`2026-09-19-agent-activity-authority.md` without changing hook ownership.

## Revisit when

A verified provider contract supplies stronger attention/resumption evidence, or
captured redraw sequences show that the consecutive-snapshot boundary loses real
forms. Reassess with those snapshots while preserving the distinction between
revoking screen evidence and declaring a turn complete.
