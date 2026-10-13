# Keep renamed settings writers in their existing test group

## Status

Implemented; the focused contract regression passes. Native validation is pending.

## Context

PR #505 replaced a dropdown regression with the large-font completion capsule
regression. Its function was renamed, but the nextest filter and its expected
configuration still named the removed function. Both could agree while excluding
the real writer from the shared-settings group.

## Evidence

In [the macOS Intel run](https://github.com/Kuddev/pebrel/actions/runs/37537809068/job/112523316502),
the capsule case completed at 22:16:36.069 UTC after 0.177 seconds, while the CJK
font case completed at 22:16:37.014 UTC after 1.033 seconds. Their execution windows
overlap; both use `SettingsBytesGuard` and modify the same settings file. The CJK
case observed an absent saved setting instead of its selected font.

The [nextest test-group contract](https://github.com/nextest-rs/nextest/blob/main/site/src/docs/configuration/test-groups.md)
states that in-process mutexes do not isolate its process-per-test model. Only
matching group members receive the configured concurrency limit.

## Decision

Replace the stale function name in the existing `theme-studio` filter and its
existing Python contract. Also check that both named capsule fixtures exist in
their Rust source, so a future rename cannot leave the two configuration strings
silently agreeing on a deleted test.

## Rejected alternatives

- Retry the failed font test or relax its saved-value assertion: hides the race.
- Serialize all tests or the entire UI module: unnecessarily reduces parallelism.
- Change product font persistence: the confirmed defect is fixture scheduling.

## Consequences

The same two capsule settings writers regain their existing shared-file isolation.
Pure animation tests and the rest of the suite remain parallel. No product code,
dependencies, platform selection, retry count or assertion semantics change.

## Validation

The updated contract fails against the old filter and passes after replacement.
All 18 native-runner contract tests and the baseline architecture check pass.
Native platform jobs must still validate the unchanged UI cases with the fixed group.

## Supersedes

None.

## Revisit when

These fixtures gain genuinely separate settings storage or their names change again.
