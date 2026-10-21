# Separate remote-guess semantics from host acquisition latency

## Status

Implemented for review; fresh native Windows CI is still required.

## Context

The remote-guess integration fixture asserts candidate meaning and executes accepted
Git commands. It previously also required every metadata acquisition to succeed
within the product's three-second wall-clock deadline. Reserving all nextest slots
cannot guarantee that precondition on a shared native runner.

## Evidence

PR #484's Windows x64 [job 118129540418](https://github.com/Kuddev/pebrel/actions/runs/39272806311/job/118129540418)
ran 3105 tests: 3104 passed and only
`real_remote_guesses_follow_configuration_and_refspecs` failed. Its first query,
`git switch discover/o`, failed before candidate assertions. Diagnostics report
`for-each-ref`, a remaining budget of 2926 ms, `Interrupted`, and an expired overall
deadline. This test already reserved `num-test-threads`; Linux, macOS and Windows
ARM native jobs passed. The other red required contexts reported the native failure.

## Decision

Separate repository reading from its bounded production transport, and candidate
calculation from snapshot acquisition. The remote-guess semantic fixture supplies
actual Git configuration and reference output through the shared repository reader.
It retains the production cache, parsing, matching, invalidation and accepted-command
assertions, without imposing an interactive deadline on fixture setup.

Production still uses the same three-second total deadline, 1 MiB output bound,
cancellation, process-tree cleanup and two-second cache lifetime. Existing request
and real-window tests still exercise that transport. No timeout increase, retry,
ignored assertion or scheduling change is added. Deterministic tests distinguish
failed acquisition from a successful empty snapshot, retain that fixture snapshot
for reuse, and reject caching a successful read after cancellation.

## Rejected alternatives

- Increase the product deadline: a runner's speed does not justify changing the
  interactive contract.
- Retry or accept empty candidates: hides failed acquisition or weakens semantics.
- Reserve more nextest capacity: the fixture already owns all configured slots.
- Replace Git with canned candidates: loses the real configuration/refspec and
  accepted-command checks. Only acquisition timing is separated here.

## Consequences

The remote-guess fixture proves semantics over real Git metadata, not three-second
availability on every host. Its setup uses the existing synchronous Git fixture
helper; native test-runner lifetime limits still apply. Other integration fixtures
retain bounded production acquisition, so this is not a suite-wide availability fix.

## Validation

A temporary source harness compiled the actual Git completion, tracking and process
reader modules with Rust 1.97.1 and host-scope adapter stand-ins. Seven targeted
semantic, acquisition/cancellation and process-reader tests passed. With a one-time
3.2-second delay before `for-each-ref`, the original remote-guess fixture failed at
its first query after 3.06 seconds; the revised fixture passed in 3.38 seconds with
all assertions intact. No retry was used.

The broader isolated run hit an unrelated nested-tag assertion on local Git 2.43.0;
the unmodified HEAD reproduces that same failure. It was not changed. Architecture
checks against PR base `8de5331f5607a24c72a8cf209ab4099c8ba4c424`, workspace formatting
and whitespace checks passed. This is not full application/native-matrix validation
and does not replace fresh hosted Windows CI.

## Supersedes

Supersedes only the requirement that the remote-guess semantic fixture acquire its
metadata under the product deadline in
[the bounded-fixture scheduling decision](../../scripts/ci/2026-10-04-bounded-git-semantic-fixtures.md).
Its scheduling reservation and all other fixture policies remain unchanged.

## Revisit when

The native acquisition tests demonstrate an actual transport defect, or a different
fixture needs the same separation between semantic correctness and host latency.
