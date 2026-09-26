# Hidden terminal output and view invalidation

## Status

Implemented locally; behavior regressions and product compilation passed. Review
is pending.

## Context

Terminal output continues while another tab, a settings surface, or a zoomed
split occupies the workspace. The terminal model must keep consuming output,
but the hidden grid does not need to invalidate the visible workspace for each
output batch. Keyboard focus cannot answer whether a split remains visible.

## Evidence

- The tab presentation reads terminal entities for labels and task state. GPUI
  therefore tracks some hidden entities as dependencies of visible chrome.
- The event mailbox already coalesces Wakeup before delivery. Nevertheless,
  `TerminalView::process_event` previously notified on every delivered Wakeup.
- A Wakeup also flushes pending shell commands and runtime Enter barriers.
  Discarding the event, or returning before those calls, would stall input.
- In the pinned GPUI, invalidating an entity during drawing does not emit the
  ordinary post-draw notification effect. The workspace render regression
  rejected an initial direct-notify implementation when revealing a pane.

## Decision

The workspace declares output visibility on each terminal entity using its
existing tab, settings, split and explicit window-hidden state. Selection of the
primary pane is shared with the actual single-pane/zoomed rendering path.

Gate only the pure output notification. Keep the model, command barriers and
semantic events running. A new entity defaults to visible until the owner first
declares its state. Revealing a hidden entity defers one invalidation until after
the current draw, including when output stopped before the pane became visible.

The state is a boolean owned by the existing UI entity. No new global registry,
thread, persisted preference or protocol is needed. Workspace terminal event
routing and presentation activity live together in `workspace/terminal_activity`.

## Rejected alternatives

- Gate on keyboard/window focus: visible unfocused splits would stop updating.
- Stop the event pump: model, command submission and exit/attention semantics
  would stop with it.
- Treat mailbox coalescing as sufficient: one event per batch still causes
  sustained hidden output to invalidate the visible window.
- Notify synchronously while declaring visibility during render: GPUI can absorb
  the notification in that draw, so it does not reliably invalidate cached views
  after reactivation.
- Add a process-wide visibility map: it duplicates ownership and requires
  cleanup and cross-App isolation that a view-owned field avoids.

## Consequences

Visibility declaration visits the workspace's panes once per workspace render,
without allocating another collection. Hidden output still incurs necessary
terminal parsing and semantic processing. Existing status/Agent notifications
and timers can still redraw the chrome; this is not a promise of zero background
work or a measured reduction in total process memory.

Native minimization, resource eviction and decoder memory budgets remain distinct
lifecycle concerns. This change does not replace their policies.

## Validation

`terminal/view/output_tests` covers repeated hidden output, current grid contents,
reactivation, unfocused visible output, redundant declarations, semantic events
and both pending command barriers. `workspace/terminal_activity::tests` exercises
the real workspace render path across tab, split, zoom, settings and explicit
hidden-state transitions. The 50 terminal view tests passed (including the four
new output regressions), and the separate workspace render regression passed.
Changed Rust files pass formatting and whitespace checks. The actual product
configuration passed `cargo check -p nebula --bin pebrel --features gpui-shell
--tests --locked --offline`. Its first run exposed a missing test-support feature
guard on the existing scientific renderer's native regression; that test is now
compiled only with the required support. These checks do not establish a process
memory reduction.

## Supersedes

None.

## Revisit when

The workspace gains simultaneous tab presentation or a different view-cache
lifecycle. Extend the visibility declaration and render regression together.
Use process-level measurements before attributing a specific byte saving to this
invalidation policy.
