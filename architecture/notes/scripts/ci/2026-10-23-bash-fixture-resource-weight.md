# Reserve runner resources for the wrapped Git Bash input fixture

## Status

Proposed scheduling correction; native Windows validation is required before merge.

## Context

The wrapped-input fixture uses the real product ConPTY and VT parser with Git Bash.
It sends a burst of 72 negotiated key-down/key-up pairs, checks Home/End movement,
and erases the input in both byte and UTF-8 locales. Each observation retains its
ten-second deadline. This is a functional protocol test, not a concurrent shell
startup benchmark.

## Evidence

The same unchanged fixture failed on Windows ARM in
[PR #707 run 39948649224](https://github.com/Kuddev/pebrel/actions/runs/39948649224)
and [PR #737 run 39949613934](https://github.com/Kuddev/pebrel/actions/runs/39949613934).
At the deadline, the C-locale backspace burst had left 15 and 11 input characters,
respectively. Passive observation for the next two seconds, with no new input,
reached the required empty line and cursor column 2. The original assertions
still failed. Other real Bash and PowerShell fixtures ran concurrently.

This establishes delayed completion rather than a tail released only by a new
key. It does not by itself prove CPU contention or exclude an input-path defect.
The scheduling change tests whether the original assertions pass when this
fixture no longer competes with other tests. A failing isolated run must still
be investigated rather than retried or given a larger deadline.

## Decision

Use nextest's existing `threads-required = "num-test-threads"` policy for only
the exact wrapped-input test name. This reserves all test slots while it runs:
<https://nexte.st/docs/configuration/threads-required/>.
No test inputs, protocol code, observation deadlines, assertions, retries or
platform coverage change. Other terminal tests keep their existing scheduling.
The configuration contract requires the exact override and the real fixture name.

## Rejected alternatives

- Raise the ten-second deadline or reduce the burst: changes the behavior checked.
- Skip ARM or retry until green: hides the unresolved failure.
- Serialize all terminal tests: delays unrelated tests without evidence.
- Change production input encoding: the recorded delayed completion does not
  justify altering the protocol.

## Consequences

The one fixture occupies the runner's test slots during both locale scenarios.
The full suite still runs, including native console, translated-byte and negotiated
input coverage. This scheduling choice gives no application latency guarantee.

## Validation

The Python configuration contract rejects extra overrides, changed filters and
changed resource weights. Native CI must pass the unchanged Windows fixture and
the complete five-platform matrix before merge. Local Linux checks cannot validate
Git Bash/ConPTY behavior on Windows ARM.

## Supersedes

None. Extends the exact-fixture approach recorded in
[the Git fixture note](2026-09-30-git-fixture-resource-weight.md).

## Revisit when

The isolated fixture still misses its deadline, or profiling demonstrates that
the current native runners can execute it reliably with competing process tests.
