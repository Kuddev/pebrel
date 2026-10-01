# Native font-control fixture isolation

## Status

Implemented using the existing shared-settings fixture contract.

## Context

Rendered settings tests use the real settings path. Their in-process mutex protects
`cargo test` callers, while nextest launches separate processes and requires the
matching `theme-studio` serial group. Readers and snapshot restoration can race
with writers when either half of the fixture contract is missing.

## Evidence

The native suite on candidate `8bd3d23d` observed interface size 17 in the pane,
then size 14 in the saved file. The font-picker layout fixture also observed a
menu overlapping its trigger. The new font-size and Chinese-font fixtures held
the mutex but were absent from nextest's group. The existing long-segment fixture
was also absent, and the font-field layout fixture lacked both forms of isolation.

See [fixture ownership](../../../../../nebula_app/src/gpui_shell/settings_fixture.rs)
and the [failed native run](https://github.com/Kuddev/pebrel/actions/runs/36885846273).

## Decision

Put these four named fixtures in the existing serial group. Add the mutex and
settings-byte snapshot to the font-field layout fixture, which can persist a font
choice when its dropdown closes. Keep the remaining suite parallel and retain all
interaction, popup geometry and saved-value assertions.

## Rejected alternatives

- Removing saved-file or popup-overlap assertions would conceal the interference.
- Retrying or increasing timeouts does not isolate a shared configuration file.
- Serializing all GPUI tests would reserve resources for unrelated fixtures.
- Adding production state guards does not prevent another test process restoring
  settings bytes behind the fixture.

## Consequences

The font controls now follow the same two-part isolation rule as theme Save/Apply
and Ctrl+wheel tests. New shared-path readers or writers must join both the mutex
and nextest group; an in-process guard alone is insufficient.

## Validation

The native workflow contract checks the precise serial-group membership while
preserving the existing independent-test and heavy-Git scheduling policies.
The original native interaction and persistence regressions remain in the suite.

## Supersedes

None; this restores the documented fixture contract.

## Revisit when

Each settings fixture receives an independent configuration directory and theme
library, eliminating the shared filesystem boundary.
