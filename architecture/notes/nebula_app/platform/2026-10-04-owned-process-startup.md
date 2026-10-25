# Attach Windows process ownership before execution

## Status

Implemented. Native validation accompanies the review.

## Context

Bounded metadata probes and pane execution share the process-group adapter.
Windows uses a kill-on-close Job Object so cancellation and timeouts also reap
descendants. Previously the child could run between `spawn` and job assignment.
A short command could exit during that interval; attaching its job then failed
and discarded otherwise valid output.

The full main Windows suite reported unavailable Git metadata before its
candidate assertions. Retrying the fixture did not resolve the repeated failure.
The original failure discarded the operating-system error, so it cannot establish
that this race explains every unavailable read.

## Evidence

An owned native Windows probe assigned a job successfully to a live child.
Assignment after a fast child's successful exit returned error 5 even though its
captured output was intact. The production adapter treated that assignment error
as a failed acquisition.

The regression in [`process.rs`](../../../../nebula_app/src/platform/process.rs)
delays ownership attachment and requires the child to remain alive with no output.
Only attachment may release execution; the test then checks its exit and output.

## Decision

Create owned Windows children with `CREATE_SUSPENDED` and the existing hidden
console flag. Establish the kill-on-close job first, then locate and resume the
new child's initial thread using the existing ToolHelp API dependency. Thread
selection is restricted to the PID of that still-owned child. Close every native
handle and fail the acquisition if attachment or resumption fails. The callers
retain their kill-and-wait error cleanup; a job guard covers resumption failure.

This changes only the shared owned-process adapter. Interactive terminal startup
and Unix process groups keep their existing paths. Git assertions require a
successful metadata read directly, without a fixture retry wrapper.

## Rejected alternatives

- Retry Git assertions or extend their deadlines: hides failed process ownership
  and does not prevent a descendant from escaping before attachment.
- Ignore job assignment failure after exit: cannot prove that no descendants were
  created before the parent exited.
- Resume before assigning the job: retains the same ownership race.
- Use nightly-only process-spawn APIs: changes the pinned stable toolchain for an
  operation supported by the existing native adapter.

## Consequences

Each owned Windows spawn has an additional thread snapshot on a background path.
No snapshot or process wait occurs in a render callback. Successful attachment
releases the child once; failed setup retains cleanup ownership. Output bounds,
timeouts, cancellation and separate stdout/stderr capture remain the caller's
responsibility.

## Validation

The native Windows ownership regression and existing process/cancellation suite
pass on an isolated desktop. Git metadata acquisition, pane execution, remote
window acceptance and selected native CI results accompany the final review.
The independent probe and regression establish the race mechanism; they do not
replace a fresh full Windows run of the formerly failing metadata fixture.

## Supersedes

None.

## Revisit when

Stable Rust exposes the initial thread handle or process attribute-list spawn,
or a measured background-spawn cost requires a narrower native implementation.
