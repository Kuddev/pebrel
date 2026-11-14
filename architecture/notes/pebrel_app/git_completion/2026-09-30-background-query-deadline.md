# Background Git lifetime and candidate availability

## Status

Implemented after reproducing candidate loss on the native Windows x64 runner.

## Context

The first semantic source used a 750 ms subprocess polling deadline. Calculation
already runs outside the UI thread, with input-owned cancellation and stale-result
checks. Treating a short interaction target as the hard child lifetime can discard
a valid query before it produces any candidate.

## Evidence

Windows x64 repeatedly failed the first inline case in
`git_completion_real_repository_reaches_all_modes_and_preserves_quoted_edits`.
The accepted bytes contained a literal Tab. Additional diagnostics in CI run
36680633475 established that the captured input, local environment and repository
directory were correct, but `read_cancellable` had exceeded its time/output limit
and the source cached an empty result. This fixture contains two short branch refs,
well below the 1 MiB output cap, so the time limit was the relevant failure.

The same test passed locally with both the standard harness and CI's nextest
version. Linux, macOS and Windows ARM also passed. Those results alone did not
establish the cause; the failing runner's source diagnostic did.

## Decision

Allow this background Git child up to three seconds before forced cleanup. This
is a hard lifetime bound, not a delay before returning and not a UI responsiveness
guarantee. Successful queries return immediately. Input changes still cancel the
old task; process polling, process-group cleanup, bounded output, cache scope and
stale-result rejection are unchanged.

## Rejected alternatives

- Retry CI until green: the same assertion failed repeatedly.
- Weaken the acceptance assertion or pre-seed synthetic candidates: that would
  remove evidence that actual discovery reaches the input path.
- Serialize the entire test suite: this would conceal a product source that also
  loses candidates when a real host query is slow.
- Wait for Git in the key handler: responsiveness belongs to the asynchronous
  request boundary, not to a synchronous timeout.

## Consequences

An uncancelled slow query can retain its worker/child longer; it remains bounded.
Normal local-query latency and the two-second snapshot reuse policy are unchanged.
No test retry, gate budget, concurrency setting or platform exception is added.

## Validation

Use the original failing native-runner test, existing source cancellation and cache
tests, and the real-window three-mode acceptance. Report platform results and native
desktop interaction separately. A successful local run is not proof that the
runner-specific candidate loss has been addressed.

## Supersedes

Supersedes only the 750 ms child deadline in
`2026-09-30-local-branch-context.md`.

## Revisit when

Measured source costs call for different task admission or cache policy, or this
bound proves insufficient despite preserved UI cancellation.
