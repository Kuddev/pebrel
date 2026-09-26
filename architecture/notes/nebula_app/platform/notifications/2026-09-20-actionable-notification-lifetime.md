# Notification actions share live confirmation identity

## Status

Proposed for review with implementation.

## Context

Completion notifications covered more cases than questions and failures. The
shell exit code was discarded before notification selection. Confirmation buttons
recognized only a trailing Y/n prompt and wrote text plus bare CR in one batch.
Native Windows notifications had no answer actions and did not retain their native
object beyond the delivery call.

## Evidence

`terminal/view/runtime.rs` already receives the shell exit code. Codex 0.154.0's
request_user_input handler submits the current question when a numbered option is
pressed; an extra Enter can submit the next question. Its approval UI advertises
configured one-key shortcuts. The terminal's shared keyboard encoder already
supports legacy VT, kitty and ConPTY control-key protocols.

## Decision

Retain up to 64 native notifications on one MTA worker with a bounded request
queue. Native action callbacks enqueue pane/request/choice events onto the UI
thread. Both in-app and native buttons resolve the same live confirmation before
writing input. Source pane, session, complete visible prompt, terminal mode and
request generation prevent old buttons from answering a different request.

A short bounded retry allows hook delivery to precede the interactive form's
render. Completion, new requests, manual input and view destruction cancel stale
work. Numbered Codex questions send a single advertised choice; approval buttons
use their displayed shortcut. Binary text and Enter are separate writes; a
lifecycle change or intervening input cancels the pending Enter. Multi-question
forms can publish the next question once a new frame is visible.

Short nonzero command exits produce a failure notification. Successful commands
retain the duration threshold, and hooked Agents retain their completion owner.
Each actionable request has its own throttle identity so answering one question
does not suppress a different question from the same pane.

## Rejected alternatives

- Unconditionally send `Y\r`: wrong for numbered choices and negotiated input.
- Add Enter after a Codex digit: can act on the next question.
- Trust only the original notification's text: stale OS notifications can survive
  a session, prompt or terminal-mode change.
- Keep one worker per toast: duplicates thread and COM lifetime management.
- Assume showing a toast proves its click callback survives: separate lifetimes.

## Consequences

Unknown/freeform forms open the source pane without guessing an answer. Native
Windows toasts display at most five actions. The bounded retained set limits
activation support for very old Action Center entries; terminal state validation
is still required for every delivered callback. UI queue acceptance is not proof
that an arbitrary third-party CLI accepted an answer.

## Validation

State tests cover stale requests, displayed shortcuts, unsupported forms and
single-question submission. GPUI terminal tests inspect actual PTY writes under
legacy and negotiated keyboard modes, cancellation and repeated clicks. Native
XML tests reject terminal controls/markup injection. Native Windows notification
click, foreground policy, visual layout and packaged-runtime acceptance remain
platform integration checks and are not established by compilation alone.

## Supersedes

None.

## Revisit when

Providers expose a documented structured answer transport or the platform offers
an owned cross-platform notification activation service.
