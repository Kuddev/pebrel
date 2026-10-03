# Pane position exchange

## Status

Proposed for review; remote validation pending.

## Context

Issue [#355](https://github.com/Kuddev/pebrel/issues/355) requests moving a split
pane to another occupied position. Existing header dragging only detaches a pane
as a new tab when it leaves the terminal area.

## Evidence

`pane_header.rs` owns the existing grip, movement threshold and pointer overlay.
`SplitTree` owns layout topology and ratios. Views are looked up by leaf ID, so
exchanging leaf IDs retains pane entities and their PTY ownership.

## Decision

Exchange source and destination leaf IDs on release inside another pane. Preserve
all split directions, ratios, pane metadata, broadcast state and source focus.
Reuse the existing grip and dock hit geometry; terminal text and header buttons
remain outside the gesture's initiation area. Escape cancels before input actions.
Resolve the source tab from its pane identity at release, and cancel when either
pane disappears. The shared `activate_tab` transition cancels synchronously on
switching, so keyboard navigation away and back cannot revive the gesture. Outside
drops retain tab extraction.

## Rejected alternatives

- Closing and recreating panes would terminate live PTYs and lose terminal history.
- Removing and re-docking the source would unnecessarily change unrelated geometry.
- A second drag system would duplicate capture, threshold and extraction behavior.

## Consequences

Dragging exchanges occupied positions. It does not insert a pane between others
or automatically equalize same-direction splits. Those remain separate #355 needs.
No persistence format or runtime-hub identity changes.

## Validation

Shared-tree regression covers mixed axes, unequal ratios, missing targets and
unchanged unrelated positions. GPUI mouse regression covers header drag, Escape,
entity and focus retention, terminal-body initiation, closing the source and
keyboard tab switching away and back before releasing.
These tests are authored; they require fork Actions and native visual acceptance.

## Supersedes

None.

## Revisit when

An approved interaction requires insertion or rebalancing rather than exchange.
