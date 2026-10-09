# Run local Git metadata probes concurrently

## Status

Implemented. The three targeted Windows x64 Git completion fixtures pass locally; fresh hosted Windows CI remains pending.

## Context

A cold local Git completion query reads selected configuration and refs under one three-second request deadline. These read-only operations were launched sequentially, so their process startup and execution costs accumulated. The existing nextest resource reservation prevents competing test work but does not reduce the cost within one query.

The hosted run for PR #542 reported all three semantic fixtures unavailable at the `config` or `for-each-ref` stage with `expired=true` on Windows x64 (run [37757638083](https://github.com/Kuddev/pebrel/actions/runs/37757638083), job [113246207164](https://github.com/Kuddev/pebrel/actions/runs/37757638083/job/113246207164)). The scheduling-only mitigation already existed in `.config/nextest.toml` and did not prevent this run's failures.

## Evidence

Before this change, the three targeted Windows x64 tests passed locally in 15.97 seconds. After parallelizing the two metadata probes, they passed in 13.69 seconds, then in 12.06 seconds on the final implementation. A one-test-thread run also passed in 21.93 seconds. These test-suite timings are directional only, not a performance guarantee.

## Decision

Launch the host-local `git config` and `git for-each-ref` reads concurrently against the same absolute deadline and cancellation callback. Keep the aggregate output limit at 1 MiB and accept the repository snapshot only when both reads succeed. WSL and SSH execution remain sequential. Each child retains the existing process ownership, timeout and cleanup path.

## Rejected alternatives

- Extending the three-second production deadline changes completion latency for every host.
- Retrying or accepting unavailable metadata would hide acquisition failure and weaken semantic assertions.
- Resource reservation alone was already present when the hosted x64 failures occurred.

## Consequences

A local cache miss uses two bounded workers and two owned child processes at once. Cache keys, invalidation, fallbacks, remote execution and the shared Git semantics are unchanged.

## Validation

The targeted Windows x64 tests pass after the change, and the current-tree architecture checker passes. Full native CI has not been rerun, so the hosted failure scenario remains to be confirmed on its runner.

## Supersedes

None. Complements the test-scheduling rationale in [`scripts/ci/2026-10-04-bounded-git-semantic-fixtures.md`](../../scripts/ci/2026-10-04-bounded-git-semantic-fixtures.md).

## Revisit when

A fresh Windows x64 native run provides evidence about acquisition reliability under the hosted runner's load.
