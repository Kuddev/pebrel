# Select native PR validation by changed paths

## Status

Implemented for review following the maintainer's 2026-09-29 request to keep
ordinary PRs fast. GitHub rulesets and required check names are unchanged.

## Context

Requiring every PR to finish five native platforms makes a small shared-code
change wait for the slowest architecture. Cache improvements do not remove that
dependency. Documentation changes need the repository contracts, but do not need
to rebuild and execute the desktop application on five hosts.

## Evidence

In [run 36585129635](https://github.com/Kuddev/pebrel/actions/runs/36585129635),
Linux completed in 4m58s, Apple Silicon in 7m03s, Windows x64 in 12m42s and
Windows ARM64 in 15m05s. Intel macOS took 25m07s.
These are one run's measurements, not promised future durations.

The branch requires five `Tests (...)` and two `Release workspace (...)`
contexts in addition to lint, architecture and PR size. Omitting dynamic matrix
rows without reporting the policy would leave required contexts missing.
GitHub evaluates a job's `if` before its matrix, so documentation-only plans can
skip an empty native matrix without allocating native runners.

## Decision

`scripts/ci_plan.py` remains the only platform catalog and selector. Ordinary
shared-code PRs run the existing complete Linux native suite. Platform paths add
the relevant native architectures. Toolchain, dependencies, CI, packaging,
cross-platform host boundaries and unknown code roots select the full matrix.
Documentation-only PRs retain lint and architecture contracts without native jobs.
Draft and ready PRs use the same policy.

The planner reads the complete merge-base diff from validated base/head commits.
Missing commits and failed Git commands fail lint instead of reducing coverage.
Renames are expanded into deletion/addition paths so moving a platform source
cannot erase its original platform requirement.
Changed Rust files are also inspected at both revisions for OS and architecture
conditions. A Windows-only function inside a generic `welcome.rs` path must still
request Windows validation when its body changes. Architecture conditions request
all native targets. This conservative inspection may also match example comments.

Native jobs use `Native tests (...)` names. Small Ubuntu reporting jobs retain the
seven required test/release context names and state whether a platform executed
or was not scheduled by the PR policy. Every selected native job must succeed;
failure, cancellation or an unexpected skip cannot produce passing reports.
Only a genuinely empty plan accepts a skipped native matrix. These reports do not
reuse earlier runs' conclusions or claim that unscheduled tests executed.
Reports use `always()` once lint has produced a valid plan: cancelling native
validation must produce failed reports, not skipped required contexts that GitHub
accepts. A failed or cancelled lint already blocks merging on its own.

Main pushes, daily scheduled runs, merge groups and manual/reusable workflow
calls keep all native platforms and release-profile checks. Existing release
packaging and platform conformance jobs are unchanged. The daily run provides a
full validation opportunity even when consecutive main pushes cancel older runs.

## Rejected alternatives

- Remove only Intel macOS: ordinary PRs would still wait for both Windows hosts.
- Skip expensive steps after assigning native runners: queue time remains.
- Remove native suites or weaken their test assertions: changes scheduling rather
  than what the selected platform proves.
- Remove required contexts from server settings: unnecessary for this policy and
  would alter the existing branch protection configuration.
- Infer safety from incomplete API file lists or failed diff reads: can silently
  omit the very platform changed by a large or renamed-file PR.

## Consequences

Ordinary PRs no longer wait for unrelated native hosts. Shared-code regressions
specific to another OS or architecture can be discovered after merge; the policy
deliberately exchanges earlier universal coverage for a shorter PR feedback loop.
Path boundaries are conservative, but are not a proof of semantic independence.

## Validation

Existing planner and workflow suites cover ordinary, documentation, Windows,
macOS, dependency and unknown-path plans, draft/ready equivalence, actual Git
renames, invalid/unavailable commits and required-report failure propagation.
Actionlint validates the workflow. Hosted execution and timings must be confirmed
on the final policy PR and subsequent ordinary PRs; local tests do not establish
a hosted-runner speed guarantee.

## Supersedes

The all-PR platform scheduling in
[shared native PR caches](2026-09-25-shared-native-pr-cache.md).
Its cache identities, complete suite commands and fixture isolation remain.

## Revisit when

Post-merge platform failures identify a missing boundary, the supported platform
set changes, or measurements show another part of the PR path dominates latency.
