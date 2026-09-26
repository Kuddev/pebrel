# Speed up native PR checks without dropping platform coverage

## Status

Implemented for review. The five native check names and two macOS release-check
names are unchanged. Hosted-runner measurements are pending the independent PR;
this change does not modify GitHub rulesets or claim a fixed completion time.

## Context

PR speed must improve without omitting native architectures, doctests, production
features or release-profile compilation. Draft PRs now have the same coverage as
ready PRs. Existing test assertions and failure propagation remain in force.

## Evidence

In [run 36119856851](https://github.com/Kuddev/pebrel/actions/runs/36119856851),
the Intel Mac job took 23m55s, including 12m59s of test compilation after both
cache layers missed. In
[run 36084876307](https://github.com/Kuddev/pebrel/actions/runs/36084876307), its
queue alone took 109m24s. Queue time and test execution are different bottlenecks.

Inspected native target snapshots were approximately 1–1.37 GB. Saving a new
snapshot for every PR commit consumes repository storage even though a sibling
PR cannot restore that merge-ref cache. GitHub documents this scope boundary and
the default cache limit in its
[dependency caching reference](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).
Default-branch caches can be restored by PRs.

Nextest 0.9.146 publishes native Windows ARM64 and x64 binaries and a universal
macOS binary. Its CLI supports the current Cargo profile and timing options.
[Nextest](https://nexte.st/docs/running/) runs unit/integration tests separately
but does not replace Cargo's doctest execution.

The first hosted nextest run, [Linux job 108135574634](https://github.com/Kuddev/pebrel/actions/runs/36154433909/job/108135574634),
ran 2,245 tests and exposed six theme-studio fixture conflicts. Those tests already
share `lock_theme_studio` because they read and restore the same settings file.
An in-process mutex does not protect separate nextest processes. Nextest's
[test groups](https://nexte.st/docs/configuration/test-groups/) provide the
corresponding cross-process scheduling boundary.

## Decision

- The existing validated platform catalog selects all five native platforms and
  both macOS release checks for draft and ready PRs. Keep the lint prerequisite,
  unfiltered PR triggers, native Windows ARM compiler assertion and SDK checks.
  Readiness alone no longer reruns the matrix: its commit already receives full
  coverage on `opened`/`synchronize`, and `reopened` still revalidates the PR.
- Native CI installs pinned nextest, runs the complete workspace without test
  filters or automatic retries, and explicitly runs `cargo test --doc` with the
  same profile/features. Keep the independent production-feature check. The
  script's default remains Cargo for existing local and release callers.
- Give the entire existing theme-studio fixture module one nextest group with
  `max-threads = 1`, including readers and writers. This restores its previous
  mutual exclusion without serializing other tests or changing assertions.
- Native jobs restore caches on PRs but publish only on the default branch.
  Use a fixed `dependencies-v1` snapshot suffix together with the existing
  compiler, OS, architecture, workload, SDK, flags and manifest identity. Source
  changes still run Cargo and all tests; cache hits never substitute results.
- Save the immutable native snapshot only after successful checks. A failed
  first attempt must not permanently publish an incomplete snapshot under the
  fixed key. Dependency/toolchain changes create new keys; deliberate generation
  changes can refresh an otherwise unchanged snapshot.
- Share Cargo downloads between architectures of the same OS. Before publishing,
  fetch both supported Windows or macOS targets. Keep compiled targets separate.
  Read-only PR consumers fetch only what their actual commands require. Restore
  old per-architecture downloads during the cache transition.
- Cancel obsolete main validation runs as well as obsolete PR/merge-group runs.
  Do not add more macOS runner jobs or change required status contexts.

## Rejected alternatives

- Omit draft, docs-only or platform-specific PR tests: violates the required
  cross-platform coverage.
- Drop doctests or the production feature graph when adopting nextest: they
  cover contracts not replaced by its unit/integration test execution.
- Remove macOS release checks or fold them back into the long native test job:
  they remain required, and prior execution measurements motivated parallelism.
- Cache a new full target directory under every source SHA: duplicates large
  snapshots while merge-ref scope prevents reuse by other PRs.
- Add sccache and incremental compilation together: sccache's Rust cache requires
  incremental compilation to be disabled and does not cache executable linking.
  This change leaves the existing tested compiler profile intact.
- Retry failing tests until green or copy earlier check conclusions: obscures
  current-commit failures rather than making validation more efficient.

## Consequences

There are still seven native runner jobs for every PR. Drafts request more jobs
than the superseded policy, not fewer. Shared caches and parallel test execution
target avoidable work; hosted-runner capacity still determines queue latency.

The first successful default-branch run seeds each new snapshot. A dependency
change can still cause a cold build. Release/package callers retain SHA-based
target snapshots; this PR does not change their build or publication semantics.

## Validation

The existing planner, native-workflow, cache, stable-release, platform-cfg and
PR-size contract suites ran 58 tests locally: 57 passed, one POSIX-only summary
execution test was skipped on Windows. Tests cover full draft/ready matrices,
read-only PR cache behavior, cross-architecture download completeness, retained
doctests/features and failure propagation at each command.

Actionlint 1.7.12 passed for the native workflow. The architecture check passed
against the PR base without policy/budget changes. Nextest ran all 71
`nebula-settings` tests successfully on Windows; its discovered test names match
Cargo's inventory exactly. Rustfmt and the subset's doctest command also passed
(that subset has zero doctests). Full native/GPUI execution,
cache restore/save behavior and cold/warm timing comparisons are validated by
the hosted PR runs, not inferred from the local subset.

## Supersedes

The draft coverage reduction in
[native matrix planning](2026-09-23-native-matrix-planning.md). Its validated
event planning and required lint ownership remain unchanged.

## Revisit when

Measured cache hit rates, archive transfer costs, nextest process overhead or
runner queue times justify a different bounded cache or test scheduling policy.
