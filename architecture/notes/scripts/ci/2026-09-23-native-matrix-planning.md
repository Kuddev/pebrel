# Plan the native matrix before requesting runners

## Status

Implemented for review. Preserves the existing ten required check names and
ready-PR platform coverage; server-side rules are not changed.

## Context

The initial draft tier skipped individual steps on Intel Mac and Windows ARM64
jobs. GitHub still assigned runners before skipping those steps. In PR #257's
draft run 35820869856, the Intel test job waited 309 seconds before spending
eight seconds on a job whose test steps were skipped.

## Evidence

`runs-on` belongs to a job, while the old draft conditions belonged to its steps.
Avoiding the platform command does not avoid allocating that job's runner.
The required `lint` check already runs on Ubuntu and can validate both formatting
and the platform plan before expensive jobs are created.

## Decision

`scripts/ci_plan.py` is the authority for platform selection. It reads the event
JSON and produces native-test and release-compile matrices. Draft PRs select the
three existing core platforms and the Apple Silicon release check. Ready PRs,
main pushes, merge groups and release callers retain all five native platforms
and both macOS release checks.

The existing required `lint` job owns planning, after rustfmt. Native and release
jobs depend on that job and consume its validated outputs. Invalid input fails
the required check rather than silently returning an empty matrix. Checkouts
explicitly discard Git credentials; the workflow retains read-only permissions.

Draft-only deferred checks are absent, not reported as successful executions.
A ready PR must still supply every required platform result. This change does
not copy previous runs' check conclusions. Cargo caches accelerate rebuilding;
they are not evidence that the current validation succeeded.

## Rejected alternatives

- Keep step-only skipping: retains the unnecessary runner allocation.
- Move planning into an unrequired job: its failure could be less visible than
  a failure in an existing required check, especially on repeated runs.
- Remove release-profile checks with Preview packaging: `cargo check --release`
  verifies compilation and does not create installation packages.
- Mirror an earlier green check onto a new run: requires independently verified
  source, base, workflow, configuration and provenance rules; cache hits alone
  cannot establish those facts.

## Consequences

Heavy jobs begin after the Ubuntu lint/planning job. Formatting errors and bad
event metadata stop before requesting platform runners. Draft PR pages may show
deferred required contexts as missing; drafts are not mergeable, and the ready
event runs the full matrix. Ready transitions still rerun core validation.

This reduces avoidable runner requests without promising a fixed queue time.
The existing Windows diagnostic dispatch continues to use its separate workflow.

## Validation

The required `lint` job now runs the lightweight planner/native-workflow/
stable-release and cache contract tests after `cargo fmt --all -- --check` and before
`ci_plan.py` writes either matrix. The count is 7 planner tests, 10 native
workflow tests, 17 stable-release tests and 8 cache contract tests. A final local run of the exact
command completed 42 tests (41 passed, one Windows-only case skipped) in 3.341 seconds on 2026-09-23; this is a local
measurement, not a general speed guarantee. The workflow contract test checks
that this step precedes planning and that both dynamic matrices depend on the
required lint job.

Planner tests cover draft and ready PRs, all full-validation caller events,
malformed input and real command-line output. Workflow contract tests require the
existing lint check to own planning, both matrices to depend on it, draft step
guards to be absent, and release compilation to remain present. Native PR and
merge-group coverage and explicit failure propagation retain their regressions.
Actual hosted-runner execution remains subject to the PR's required CI results.

## Supersedes

Step-level draft skipping in PR #257's initial native matrix.

## Revisit when

Queue and execution measurements justify a different platform schedule, or a
separately reviewed artifact-provenance design supports build-once test shards.
