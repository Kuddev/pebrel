# Reserve runner resources for the real Git completion fixture

## Status

Implemented for review; the complete native matrix validates the scheduling change.

## Context

The completion fixture exercises three modes and three quoting forms using real
Git processes, GPUI tasks and PTY input assertions. Its application queries retain
a three-second wall-clock lifetime. Running this nine-scenario fixture alongside
other UI tests makes runner contention part of a correctness assertion.

## Evidence

In Windows x64 job `109811825765` of
[run 36692156699](https://github.com/Kuddev/pebrel/actions/runs/36692156699),
the fixture failed with `Process probe exceeded its time or output limit` and an
empty branch snapshot in Hybrid mode. Its captured input, local environment and
repository cwd were correct. Other UI tests were completing concurrently.

On the same production revision, the local sequential GPUI completion run passed
65 tests. The explicit native window/PowerShell/PTY/Git/npm test passed all eleven
scenarios. Linux and both macOS native jobs also passed. These observations support
a scheduling issue; a green rerun is still required rather than assumed.

Nextest documents `threads-required = "num-test-threads"` for a heavy test which
must run without competing tests:
<https://nexte.st/docs/configuration/threads-required/>.
The repository pins nextest 0.9.146.

## Decision

Give only this exact test a weight equal to the runner's test-thread count. Keep
the existing settings-write group separate. All other tests retain their current
parallelism; no test is excluded or retried, and no query deadline or assertion
changes. The CI configuration contract enumerates both permitted overrides so a
broad accidental serialization is rejected.

## Rejected alternatives

- Increase the production deadline again: CI load is not a product latency budget.
- Retry until green: loses the failure signal without documenting resource needs.
- Replace real Git with canned candidates: removes the source-to-PTY contract.
- Serialize the complete suite: constrains unrelated lightweight tests.

## Consequences

The fixture briefly occupies all test slots on each native runner. This may
increase total suite duration, but the bounded I/O test remains a functional
integration check. It is not a contention benchmark or a production performance
guarantee. Query cancellation, output bounds and cleanup tests still execute.

## Validation

The existing CI configuration contract checks the exact filter and resource weight
alongside the unchanged settings group. The complete native matrix must pass the
same assertions with the new scheduling before merge. No GUI source changes are
included in this scheduling correction.

## Supersedes

None.

## Revisit when

The fixture is split along meaningful lifecycle boundaries, process discovery no
longer owns a wall-clock deadline, or profiling demonstrates safe concurrency for
this fixture under the current native runner resources.
