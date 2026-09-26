# Typed turn results and pane notification replacement

## Status

Implemented for integration review; execution results will come from CI.

## Context

A provider can end a turn because its request failed or its answer was truncated.
The existing notification adapter suppressed those results entirely. Repeated
retries can also fill the visible notification stack with results from one pane.

## Evidence

- The shared [hook protocol](../../../../../nebula_app/src/ai_hook/protocol.rs)
  already parses explicit success, failure, cancellation and incomplete outcomes.
- The [notification model](../../../../../nebula_app/src/notify.rs) receives those
  typed outcomes, and both UI adapters consume its display text.
- The [GPUI adapter](../../../../../nebula_app/src/gpui_shell/toast.rs) already owns
  duration selection, three-card retention and weak-reference expiry timers.

## Decision

Failed and incomplete turns produce an issue notification. Cancellation stays
silent; an unclassified result from a provider that reports typed outcomes uses
neutral wording. A turn with active background tasks still emits no result.
Issue notifications cannot act as permission requests.

Identical failures from one pane share a fixed 30-second cooldown before either
notification channel. A successful result ends that failure episode. Different
panes and different errors remain independent.

Ordinary results reuse the pane's component notification identity, so a new result
replaces the old card while other panes remain visible. Confirmation identities,
configured durations, the existing default lifetimes and three-card retention
remain owned by their existing implementations.

## Rejected alternatives

- Inferring success from a stopped process or screen silence lacks provider proof.
- Showing a failed result as an attention request could expose unrelated actions.
- Replacing duration selection with fixed 5/30-second values would discard the
  newly integrated setting and its compatibility behavior.
- A second notification queue would duplicate the component's entity ownership.

## Consequences

Users can see a failed request without receiving a completion message. Automatic
retries share a cooldown, and a later result replaces that pane's earlier card.
Closing or replacing presentation never changes task state or submits input.
The cooldown does not replace the existing native notification throttle.

## Validation

The hook/notification regressions exercise failure, truncation, cancellation,
unknown outcomes and active background work through the production parser. The
component regression retains an old card deliberately and verifies that its timer
cannot close the replacement or change the other pane's deadline. Existing
duration and overflow regressions remain enabled. CI execution is pending.

## Supersedes

None. This extends the notification-duration and retention decision.

## Revisit when

Providers gain richer outcome metadata, or notification identities become scoped
to a session instead of a pane. Extend the delivery and lifetime regressions with
any change to those ownership boundaries.
