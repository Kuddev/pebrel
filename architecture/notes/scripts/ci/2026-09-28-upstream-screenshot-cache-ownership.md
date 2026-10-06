# Keep auxiliary screenshot work from displacing shared native caches

## Status

Operational change applied and read back on 2026-09-28. This note records the
upstream workflow registration and cache cleanup; committing documentation alone
does not change GitHub settings. No branch-protection rule was changed.

## Context

The required native suite publishes reusable default-branch snapshots and restores
them read-only on PRs. An auxiliary screenshot workflow introduced through open
fork PRs independently built complete Windows/macOS release applications and
published large PR-scoped Rust caches in the upstream repository.

The screenshot workflow is absent from `main` at `8087306b`, but its GitHub
registration still accepted events for PR branches that supplied the file. Editing
only a workflow on `main` would not change those already-open fork branches.

## Evidence

- [Native run 36327240193](https://github.com/Kuddev/pebrel/actions/runs/36327240193)
  took 85m29s. Its Intel Mac queued for 48m07s and executed for 36m47s. Both Mac
  architectures missed their native and release target caches, with the same
  keys used by previously available default-branch entries.
- The cache inventory contained four `v0-rust-native-ui-` snapshots scoped to
  PR merge refs, together using 5.39 GB. The previously observed default-branch
  Mac target snapshots were no longer present. These observations establish
  cache pressure and misses, not an audit of who deleted each older entry.
- [Screenshot run 36325527329](https://github.com/Kuddev/pebrel/actions/runs/36325527329)
  used `.github/workflows/ui-review-screenshots.yml` from a fork PR. Its source
  ran on both fork pushes and upstream PR updates; its publication job was
  explicitly restricted to the source fork's push events. Upstream repeated the
  full application builds but did not run that publication job.
- The screenshot action used `Swatinem/rust-cache` with failure caching and no
  read-only restriction. GitHub's [cache scope rules](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)
  do not make one PR's merge-ref cache available to sibling PRs merely because
  their visible cache keys match.

## Decision

- Disable only the upstream `UI review screenshots` registration, workflow ID
  `366570225`, through the documented workflow API. Leave source-fork workflow
  `366585525` active. No contributor branch or application code is rewritten.
- Preserve the full five-platform native suite, release-profile checks, required
  contexts, assertions, feature graphs, packaging and branch-protection rules.
  Auxiliary screenshot evidence remains separate from native regression coverage.
- After confirming there are no active screenshot runs, remove only identified
  upstream PR-scoped `v0-rust-native-ui-` entries. Preserve every default-branch
  cache, release artifact and source file. Do not install recurring broad cleanup.
- Keep contributor screenshot publication in its source repository. Any future
  upstream screenshot entrypoint must be explicitly requested, restore target
  caches read-only, and retain exact-source evidence before it is enabled.

## Rejected alternatives

- Delete all caches: would force more cold native and release checks.
- Raise storage quota or buy runners first: does not address duplicate upstream
  builds or the unshared snapshots that caused the observed competition.
- Push fixes into all contributor branches immediately: creates new PR heads and
  repeats their required native matrices. The upstream registration can be stopped
  without editing their feature code or interrupting their fork publication.
- Add a new scanning framework or equivalent CI gate: unnecessary for retiring
  this identified auxiliary registration. A source check alone also does not stop
  an already-running workflow from publishing another snapshot.
- Remove native platforms, Markdown regressions or assertions: changes required
  coverage rather than addressing resource ownership.

## Consequences

Upstream PR updates no longer launch this registered screenshot workflow. Native
tests still run normally, including for docs-only changes. Source-fork screenshot
builds and artifacts remain available for review with their source identity.

At cleanup time only one of the identified PR screenshot entries remained. Removing
it freed 1,351,294,893 bytes; the other previously observed entries were already
absent and are not counted as deletions performed by this change. The resulting
inventory retained all ten default-branch source/target snapshots, totaling
9,023,076,490 bytes. No cache quota or runner-plan setting changed.

This retires one upstream registration, not arbitrary future workflow files.
New auxiliary workflows and any re-enablement still need resource/ownership review.
Dependency or SDK changes can legitimately cause cold builds; cache retention does
not establish a fixed CI duration.

## Validation

- The disable API returned 204; a fresh GET reported `disabled_manually` for
  upstream workflow `366570225` and `active` for fork workflow `366585525`.
- No screenshot run was active or queued, so no workflow run was cancelled.
- Deleting cache `8179623547` returned 204. Readback showed the selected PR cache
  absent and every pre-cleanup default-branch cache ID still present.
- The main-branch rules read before and after the operation were identical.
- The preceding main native run [36362067181](https://github.com/Kuddev/pebrel/actions/runs/36362067181)
  completed successfully and repopulated the shared snapshots. A subsequent PR
  run measures reuse; this operation does not claim that the eight-minute target
  has already been achieved.

## Supersedes

None. The coverage and cache boundaries in
[shared native PR caches](2026-09-25-shared-native-pr-cache.md) and
[shared macOS runners](2026-09-27-shared-macos-runners.md) remain unchanged.

## Revisit when

An upstream-owned screenshot workflow has an agreed opt-in trigger, source identity
and bounded read-only cache use. Re-enabling the recorded registration requires an
explicit maintainer action after that review; a documentation edit is not an
enablement operation.
