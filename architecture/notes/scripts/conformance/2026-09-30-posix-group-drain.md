# Drain a POSIX launch before reaping its leader

## Status

Implemented; native validation is recorded with the change.

## Context

The conformance harness sent one process-group signal, waited for the launch
leader, then discarded ownership. On macOS Intel, rejecting a stale runtime
endpoint occasionally left a grandchild alive in that launch group. A successful
leader wait did not prove the group was drained.

## Evidence

- Native diagnostic run [36703024607](https://github.com/Kuddev/pebrel/actions/runs/36703024607)
  reproduced the failure on repetition 1004: leader 28259 exited with SIGKILL;
  grandchild 28263 remained sleeping with PPID 1 and PGID 28259. Its inherited
  stdout pipe remained open beyond the existing two-second assertion.
- Apple's [killpg1](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sig.c)
  calls [pgrp_iterate](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_proc.c).
  The iterator snapshots member PIDs before delivering signals. A descendant
  being created can miss that snapshot.
- POSIX `waitid` with `WNOWAIT` observes termination while leaving the child
  waitable. This keeps the launch leader's PID reserved until cleanup completes.

## Decision

Keep POSIX lifecycle details in `scripts/conformance/posix_process.py`. Startup
and normal-close checks observe leader exit without reaping it. Cleanup signals
the private group, waits for leader exit, checks for remaining live group members,
and repeats SIGKILL when needed. Reap the leader only after no live members remain.

Use the common macOS/Linux `ps -axo pid=,pgid=,stat=` format. Ignore zombies, whose
descriptors are already closed; their eventual reaping belongs to their parents.
The process query is bounded by the cleanup deadline and only runs on the cold
teardown path. No UI worker, service or Python dependency is added. Windows keeps
its existing Job Object ownership and exit handling.

Keep the process on timeout or inspection failure so cleanup can be retried.
Fail explicitly if some caller already reaped the leader: repeatedly signalling
a possibly reused numeric PGID is not an acceptable fallback.

Darwin's `killpg1` also returns EPERM when all remaining members are zombies
(its filter excludes zombies and leaves `nfound` zero). Accept that error only
after process inspection proves there are no live members. A permission error
with a live member still propagates and preserves ownership.

## Rejected alternatives

- Retry CI or increase the pipe assertion's timeout: the captured grandchild
  was still running, rather than merely awaiting reaping.
- Wait for the fixture tree before testing startup rejection: conceals the
  real startup/termination overlap.
- Repeatedly signal after `Popen.wait`: releases the PID reservation too early.
- Wait for `killpg(pgid, 0)` to fail: Linux zombies can keep the group present,
  including the intentionally retained leader.
- Add a permanent supervisor or logging pipe worker: unnecessary for an owned,
  waitable POSIX child and the current process-group containment boundary.

## Consequences

Conformance launches now require the system `ps` utility on POSIX hosts. Process
table failures remain visible. Private-group descendants are covered; a process
that deliberately escapes via a new session remains outside this existing
containment contract. All harness exit checks must preserve the leader until
group cleanup; direct early `Popen.wait` or `poll` is no longer valid on POSIX.

## Validation

The existing POSIX suite retains startup rejection, wrapper exit, restart,
normal-close exit codes and unrelated-process isolation. A deterministic
regression makes the first signal miss the grandchild; another checks timeout
ownership and retry. Native macOS execution remains necessary to validate the
original timing failure; Linux and Windows tests are separate evidence.

## Supersedes

None.

## Revisit when

The harness must contain session-escaping processes, runs on a POSIX host without
`waitid`/`WNOWAIT`/`ps`, or teardown requires richer process identity tracking.
