# Bound the lifetime of one-shot Hook forwarding

## Status

Implemented; pending review.

## Context

[Issue #258](https://github.com/Kuddev/pebrel/issues/258) reports an update that
first failed because `pebrel-hook.exe` remained in use after the application
closed, followed by a successful same-version repair incorrectly marked failed.
The helper must finish even when its caller or notification receiver stalls.

## Evidence

`nebula_hook/src/main.rs` drained stdin through `read_to_end` and wrote the
notification through synchronous `write_all`. Neither had a deadline. A caller
retaining stdin can prevent EOF; a receiver that stops reading can block a write.
Both conditions are reproduced by actual child processes in
[`lifetime.rs`](../../../nebula_hook/tests/lifetime.rs).

## Decision

Run the existing forwarding operation on one process-owned worker. The main
thread waits up to two seconds, then returns normally. Returning from this
one-shot process releases its worker and OS handles without leaving a resident
thread. This bounds stdin, oversize-input draining and notification I/O together.
No new dependency, background application service or platform thread is added.

The user's chained notifier is launched independently after forwarding/waiting;
it is neither joined nor terminated. Cursor's submission-allow response remains
on the main path, including timeout. All provider-visible exits remain successful.

## Rejected alternatives

- Timing only stdin leaves a blocked pipe write alive.
- Joining the worker after timeout defeats the bound.
- Terminating every process named `pebrel-hook.exe` can affect another installation
  or invocation. This change retires only the helper's own process.
- Skipping stdin entirely breaks providers which stream their payload into it.

## Consequences

A stalled or unusually slow invocation can lose its best-effort notification.
The forwarding wait is bounded; this is not a universal process startup latency
guarantee. Already running binaries from an older installation do not acquire
the new deadline and may still require the user to close the owning CLI/restart.

## Validation

`cargo test --locked -p nebula_hook`: seven unit tests and five actual-process
tests passed on Windows, covering retained stdin, a connected non-reading named
pipe, normal completion, Cursor continuation and the original chained notifier.
Non-Windows integration paths still require their native CI run.

## Supersedes

None.

## Revisit when

Providers require a longer documented delivery window or the transport becomes
cancellable without a per-invocation worker.
