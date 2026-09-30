# Share macOS runners without dropping native or release checks

## Status

Implemented for review. Required check names and coverage are preserved;
GitHub protection settings are unchanged. Hosted execution and timing of this
revision remain to be measured.

## Context

Separating each Mac's native tests and release-profile compile check requests
four macOS runners per PR. Parallel work can shorten execution, but an additional
runner request can also become the critical path when capacity is scarce.

## Evidence

In [run 36314579470](https://github.com/Kuddev/pebrel/actions/runs/36314579470),
the Intel release check executed for 3m16s after waiting 60m09s. The workflow
finished after 66m21s. This is a scheduling problem, not an hour of compilation.

That run's Windows ARM64 job missed its compiled target cache at 11:09:51 UTC.
The same key's currently available default-branch entry was created at 11:31:52
UTC on 2026-09-27. This establishes availability at those two observations,
not the cause of any earlier eviction or a broken cache key. Do not invalidate
existing caches as a speculative fix.

GitHub's [cache documentation](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)
states that the cached paths participate in cache versioning. Combining the
two target-directory lists would prevent reuse even if the visible key stayed
unchanged. Its [job dependency contract](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idneeds)
also requires an explicit status condition to continue after a failed dependency.

## Decision

- Run the original `cargo check --locked --workspace --release --timings` after
  each Mac's native suite on the same runner. Retain the native architecture,
  AppKit SDK requirement, pinned toolchain, test features and profile settings.
  Linux and both Windows jobs retain their existing release checks.
- Keep both `Release workspace (<os>)` names as short Ubuntu jobs. They depend
  on lint and the current native matrix, and reject any result other than exact
  `success`. They do not run Mac compilation on Linux, read older check results,
  upload fabricated statuses, or require write permissions.
- Preserve separate `native` and `native-release-check` cache keys, generations
  and target paths. Both identities are computed before build/test commands can
  generate additional manifests. The second cache action reuses downloads already
  restored by the first action; other callers keep the default download behavior.
- Save the fully successful native workload before beginning the Mac release
  workload. A later release failure or cancellation need not discard that
  completed warmup. Neither incomplete workload nor read-only PR consumers may
  publish a compiled snapshot.
- Retain all five platforms for draft/ready PRs and merge groups, both Python
  suites, nextest, doctests, the product feature check and current cancellation
  policy. Release/package callers and branch protection are not redesigned.

## Rejected alternatives

- Remove a platform, release compilation, doctests or assertions: changes the
  required coverage rather than removing scheduling waste.
- Keep requesting a separate Mac just to preserve a status name: a current-run
  dependency check retains the required context without another scarce runner.
- Introduce per-platform result artifacts or custom check-writing credentials:
  unnecessary for the conservative rule that every native job must succeed.
- Combine existing target archives or bump their generation: causes a cold-cache
  transition without changing any compiler inputs or fixing a proven defect.
- Delete remote caches or increase storage quota: not needed for this runner
  change, and not evidence that cache retention has been solved.

## Consequences

Each full invocation requests two rather than four macOS runners. The two short
result jobs still request Ubuntu runners, so this is not a reduction in the
number of named checks. Each Mac's execution becomes sequential and can take
longer on a cold cache; the change targets additional runner queue time.

Both release contexts wait for the entire native matrix. An unrelated native
failure also fails these summaries; the per-platform job logs retain the actual
compile outcome. This is conservative and does not weaken merge acceptance.
Native timing artifacts now include the Mac release compilation timings too.

Existing cache footprint, dependency cold builds and default-branch seed timing
remain relevant. This change does not promise a fixed CI duration or eliminate
Windows ARM64 compilation cost.

## Validation

The six existing lint contract suites ran 62 tests on Windows: 61 passed and the
existing POSIX-only cache-summary case was skipped. The workflow's actual Python
result-check body accepts `success` and rejects failure, cancellation, skipped,
empty and malformed results. Contracts retain full platform/event coverage,
release compilation, default-branch-only cache publication and independent
cache identities. Actionlint 1.7.12, workspace rustfmt, architecture checking
against the PR base and the platform budget passed. Hosted cache restore/save
and cross-platform timings remain pending a real workflow run.

## Supersedes

Only the separate macOS runner placement in
[shared native PR caches](2026-09-25-shared-native-pr-cache.md). Its coverage,
cache ownership and native test-runner decisions remain in force.

## Revisit when

Hosted measurements show sequential cold compilation exceeds the queue savings,
macOS capacity changes, or a demonstrated cache-retention issue warrants its own
bounded change. Compare queue, active execution and cache hit status separately.
