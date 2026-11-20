# Preserve quiescent resources for bounded Git semantic fixtures

## Status

Implemented for review. Fresh complete native CI remains required before merge.

## Context

Real Git completion uses a three-second wall-clock metadata budget. An unavailable
read is a legitimate bounded-request outcome, with native fallback, rather than a
successful empty repository. Semantic fixtures require successful acquisition to
assert candidate meaning. Competing UI work can make that precondition unavailable.

The existing resource rule protects the nine-scene window fixture, but not the
three backend Git fixtures or four request fixtures with the same acquisition
requirement. Their production queries share that deadline.

## Evidence

Windows x64 [job 111441616189](https://github.com/Kuddev/pebrel/actions/runs/37204079779/job/111441616189)
ran 2796 tests: 2795 passed, one remote-guess fixture failed for unavailable metadata
before candidate assertions, and 35 were skipped. The prior process repair's full
five-platform PR CI and subsequent main run passed; that fixed ownership race does
not establish availability under every parallel load.

A local Windows run with 16 test threads reproduced four metadata request fixture
failures. Diagnostics identify `for-each-ref`, remaining budgets of roughly
1.36–1.71 seconds, `Interrupted`, no native OS error, and an expired overall deadline.
The same application revision had passed its full four-thread suite. This proves
the load mechanism locally; the original hosted failure did not retain a raw error.

## Decision

Extend the existing nextest resource policy to seven complete test names: three
real Git semantic fixtures and four metadata request fixtures. Each occupies the
runner's test-thread capacity while active. Keep the existing settings-write group
and nine-scene fixture rule. The configuration contract enumerates every exact
override and rejects broad serialization, exclusions or retries.

Keep the production three-second budget, cancellation, output bounds, cleanup and
semantic assertions. Test-only diagnostics retain acquisition errors and stages
without printing command buffers, cwd, configuration contents or credentials.
Success fixtures still read actual Git metadata and execute their relevant effects.

## Rejected alternatives

- Extend production timeouts for a loaded test runner: changes interactive behavior.
- Retry until acquisition succeeds: obscures the actual failure and test precondition.
- Accept empty candidates: weakens semantic assertions and confuses unavailable data
  with a valid empty repository.
- Serialize the complete suite or entire completion module: constrains unrelated
  pure tests and failure/cancellation coverage without this resource requirement.
- Replace real metadata with canned candidates: removes the execution contract.

## Consequences

These seven fixtures briefly reserve all test slots; all tests still run and the
complete platform matrix remains selected. Cancellation, stale-result and failure
tests retain normal scheduling. This is a semantic integration contract, not an
availability or performance guarantee under arbitrary competing load.

## Validation

The existing CI configuration test validates the exact filters and resource weight
alongside the unchanged settings group. Actual quiescent native fixture results
and fresh complete native CI accompany the review. The failing local load run and
hosted run are retained as evidence; they are not relabeled as passing runs.

## Supersedes

Extends the scheduling decision in
[`2026-09-30-git-fixture-resource-weight.md`](2026-09-30-git-fixture-resource-weight.md)
to the semantic fixtures that require the same bounded acquisition. Its original
window-fixture rationale remains applicable.

## Revisit when

The metadata adapter can prove successful acquisition independently of wall-clock
contention, or measured native runner resources support safe fixture concurrency.
