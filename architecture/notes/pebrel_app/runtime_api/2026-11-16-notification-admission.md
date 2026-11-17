# Runtime notification admission

## Status

Implemented for review of PR #627.

## Context

Agents need to request attention without writing a command to the terminal or
changing window focus. The proposed `pane.notify` operation reused the existing
banner and native notification paths, but played the configured bell first.

## Evidence

Two identical Runtime requests could sound twice although banner admission
discarded the second message. Native notification throttling used the same pane
and non-attention identity as a task completion, so a Runtime message could
suppress an unrelated completion during the three-second native cooldown.

The reviewed routing is in `workspace/residency.rs`; banner admission is owned
by `gpui_shell/toast.rs`, and native cooldown policy is owned by `notify.rs`.

## Decision

Keep `pane.notify` additive and GPUI-only. Validate the notification's message
and optional title as bounded plain-text lines, reporting the offending field.

Runtime notifications use the existing per-pane banner admission before the
configured bell, background tab marker and native delivery. The banner helper
returns admission, including a deferred banner when its window is being set up;
it does not claim that the operating system displayed a notification.

Native Runtime messages have a separate cooldown identity from task results
and confirmation requests. The existing per-pane and request-specific identities
for non-Runtime events remain unchanged.

The command enum lives with its request parser in `runtime_api/command.rs`; the
root re-exports its existing path. This keeps the added command and its decoding
under one owner without expanding the already full Runtime hub module.

## Rejected alternatives

- Ringing before admission retains duplicate audible and visual cues.
- Copying banner deduplication into the Runtime dispatcher creates two owners
  for the same comparison and expiry rule.
- Sharing a native cooldown between Runtime and task-result messages lets one
  independent notification source consume another source's delivery window.

## Consequences

Visible panes receive an in-app banner. Background panes additionally mark
their tab and request a system notification. Bell mode controls the explicit
sound/flash cue; disabling it does not disable the message. A duplicate keeps the
usual successful Runtime receipt while producing no additional cue or delivery.

No new dependency, persistence or background service is introduced. The legacy
shell continues to reject this operation and does not advertise the capability.

## Validation

Parser regressions cover empty, control-containing and oversized messages and
titles. Native policy tests keep Runtime and task cooldowns independent.
A routed GPUI regression captures native delivery and controls admission time
while exercising the actual flash generation, duplicate requests, separate
panes, expiry, disabled bell, missing panes and hidden-window/input invariants.
Its settings and SSH inputs are isolated in a fresh child process.

## Supersedes

None.

## Revisit when

Runtime notifications gain explicit request identities or delivery receipts.
Keep admission and the distinction between accepted and displayed messages clear.
