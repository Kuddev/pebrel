# Window animation ownership behind the existing display entry

## Status

Maintainer-requested first extraction from the oversized legacy display entry,
2026-09-30. Further responsibility migrations remain separate changes.

## Context

The legacy display entry mixed rendering orchestration with window clocks, toggle
transitions and mutable animation internals. Chrome and SSH editor adapters wrote
those internals directly. Moving declarations alone would retain that coupling.

## Evidence

`display/mod.rs` had 11,060 lines before this extraction. The default GPUI product
selects `product_ui/mod.rs` instead, so its passing tests cannot validate this
legacy entry. Explicit `legacy-shell` test compilation also exposed a pre-existing
distribution test referencing the disabled GPUI updater module (seven E0433
errors). Feature-scoping just those assertions restored compilation; the common
installation-ownership assertions still execute in both feature configurations.

## Decision

Keep the existing `Display` operations as the migration entry. Move its animation
state to `display/animations.rs`, with private fields and concrete methods for
targets, progress, frame access and editor reset. Callers map settings hits to
toggle slots. The animation module does not know `Display` or hit-test enums.

Move spinner phase progression and its existing regression with the state it
updates. Rendering retains geometry and visibility decisions. Keep the current
motion runtime, curves, durations, bounded frame delta and once-per-draw calling
order. No new service, renderer, scheduler or persistence is introduced.

## Rejected alternatives

- Splitting by line count or exporting fields: neither changes state ownership.
- Replacing the complete legacy renderer: unrelated behavior cannot be safely
  verified as part of this first extraction.
- Treating default-product CI as legacy coverage: the two entry modules are
  feature-selected, so the edited path requires explicit compilation and tests.
- Removing the failing distribution test: its common and GPUI-specific checks
  remain useful when compiled with the correct capability boundary.

## Consequences

The display entry shrinks while its public methods remain stable. Views cannot
replace spring/tween state or change spinner storage directly. Existing consumers
share the same frame snapshot. The rest of the oversized display and settings
files is not claimed to be migrated.

## Validation

Before extraction, explicit legacy compilation plus four distribution, seven
chrome and seventeen display regressions passed after the feature-boundary fix.
Keep the moved spinner test and exercise pressed/released toggle channels, shared
frame ownership and editor-reset isolation in the new module. Recompile the legacy
entry, rerun affected existing tests, and verify the normal GPUI product separately.
No source budget is raised; no existing test assertion is deleted.

## Supersedes

None.

## Revisit when

Another window animation consumer needs the same state contract, or a subsequent
cohesive legacy migration removes the remaining forwarding operations.
