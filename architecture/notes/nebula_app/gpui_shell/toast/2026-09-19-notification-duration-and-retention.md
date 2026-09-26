# Notification duration and retained cards

## Status

Implemented.

## Context

Users need to choose how long in-app notifications remain visible. Existing
callers have different defaults, and hiding an approval request must never
answer it. A persistent option also removes the expiry that previously limited
how long hidden cards could accumulate.

## Evidence

The notification component owns card entities and their dismissal subscriptions.
The application renders the newest three cards, but filtering the rendered
vector does not remove older entities from the component's queue. A regression
that inserts 128 persistent cards retained all 128 after advancing the test clock
by one hour before the retention fix.

## Decision

`nebula_settings::NotificationDuration` owns accepted values, compatibility
fallback and conversion to a lifetime. The settings UI uses shared persistence
and refreshes the cached GPUI settings. Failed saves restore the saved selection
and provide visible feedback. Existing cards keep their original deadlines.

The toast adapter disables the component's fixed timer and applies the chosen
duration when a card is inserted or refreshed. Timers hold weak entity references;
persistent cards create no expiry timer. Replacements receive a new entity, so a
previous deadline cannot dismiss a replacement with the same logical identity.

After insertion, cards outside the latest three are retired through the
component's subscribed `DismissEvent`. Its normal effect processing removes both
the entity and its subscription. Retirement invokes no click, action or close
callback, matching replacement's presentation-only semantics. No hidden history
queue or second notification store is introduced. Requests remain in terminals,
and updates remain available in Settings.

## Rejected alternatives

- Change the existing default globally: it would shorten persistent update
  notices or change short-toast compatibility for users who made no selection.
- Retain hidden cards indefinitely in persistent mode: it introduces an
  unbounded backlog of entities, closures and subscriptions.
- Treat expiry as an answer to a request: a display preference cannot grant or
  deny permission or change the underlying task.
- Fork the component for configurable timers: a narrow adapter can use its
  existing entity and dismissal contracts without a dependency revision change.

## Consequences

Only the latest three cards survive notification effect processing. A burst may
temporarily create more entities inside the current update cycle; this is not a
hard process-memory limit. Cards already replaced or removed do not stay alive
because of an expiry timer. Native system notifications remain independent.

## Validation

The new burst regression fails against the pre-fix adapter. Notification tests
cover default and selected lifetimes, action independence, manual dismissal,
refreshed identities, startup deferral, overflow release and replacement before
pending removal events are delivered. The tests use the actual notification
component and a controlled clock; platform-specific execution evidence belongs
with the pull request's validation record.

## Supersedes

None.

## Revisit when

A notification history or another host bypasses the adapter. Such a feature
needs its own bounded lifetime and must preserve presentation/domain separation.
