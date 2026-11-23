# Git drawer rendering follows the viewport

## Status

Proposed for review with implementation.

## Context

Opening a Git drawer containing hundreds of changes made the GPUI workspace
expensive to repaint. An active Agent spinner schedules workspace redraws, so
otherwise unchanged repository data repeatedly incurred the same UI cost.

## Evidence

The legacy `display/side_panel/render.rs` bounds row construction with skip/take.
The shared `SidePanel` still coalesces background scans and refreshes on explicit
triggers. The GPUI drawer cloned all GitInfo vectors, constructed every change
row and rebuilt history topology inside each render. Its scroll container clipped
pixels after row construction; clipping did not bound construction cost.

## Decision

Publish an immutable Arc snapshot from the existing shared worker. Retain the
borrowed `git()` query for legacy callers. The GPUI VCS list caches descriptors
and graph topology by snapshot identity, view and repository root, and uses a
persistent variable-height GPUI ListState to construct only requested rows.

Keep operation callbacks tied to descriptor paths and section identities. Preserve
scroll position for refreshed data in the same repository/view, and reset when
switching repository or view. Use one row renderer for actual UI and layout tests.

## Rejected alternatives

- Add a faster scan interval: does not address repeated UI construction.
- Freeze background spinners: removes visible progress without fixing the list.
- Cache an entire element tree: risks stale selection, theme and operation state.
- Add a second VCS backend: duplicates existing scan ownership and coalescing.

## Consequences

Snapshot replacement does O(number of records) descriptor work once. A normal
repaint builds visible rows plus bounded overdraw. Visible rows still read current
selection, theme and operation availability. The Arc retains one current snapshot
until consumers release it. No additional watcher, polling loop or worker exists.

## Validation

The GPUI layout regression loads 237 and 5,000 changes in a 300px viewport, counts
actual row construction, scrolls to a late row and clicks its real hit area.
Grouping tests preserve conflict deduplication and staged/unstaged operation
identities. These bounded-work checks do not establish a whole-application CPU
percentage; native desktop performance and visual acceptance remain separate.

## Supersedes

None.

## Revisit when

Repository snapshots become incremental or VCS rows require different navigation
semantics. Preserve viewport-bounded construction across future shell migrations.
