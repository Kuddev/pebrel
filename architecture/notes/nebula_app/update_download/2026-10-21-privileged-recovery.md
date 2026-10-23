# Administrator updates retain their own workspace and recovery targets

## Status

Implemented for review. Native helper and GPUI regressions are separate from packaged
installation and interactive workspace acceptance.

## Context

Removing the elevated-update rejection alone still left no durable workspace:
SessionPersistence intentionally skipped ordinary persistence for administrator windows.
Scheduled startup would read the ordinary workspace, and initial-window startup rejected
administrator restore tickets. Elevated resident handover also lacked a recovery request.

## Evidence

The affected boundaries are `handoff::prepare/schedule/apply_scheduled`,
`SessionPersistence`, `run_shell`, authenticated elevation handover and the window registry.
The existing window-count acknowledgement could be blocked by unrelated window changes,
or accidentally completed by an unrelated window replacing a failed recovery target.

## Decision

Retain privileged session snapshots in memory during ordinary operation. Immediate
update commit durably saves that snapshot in the protected update transaction before
authorizing installation. Explicitly scheduling an administrator update also saves its
snapshot and arms subsequent private checkpoints. Normal administrator startup still
does not restore or overwrite the ordinary workspace.

Choose scheduled snapshots by actual privilege: administrator startup reads only its
private snapshot and treats a missing snapshot as failure. Ordinary startup retains the
existing shared-session behavior. Perform scheduled installation after elevated owner
election so a second-window launch cannot consume the existing owner's schedule.

Bind plans and recovery to actual elevation, executable, configuration, transaction and
version. Existing plans without the new elevation field remain ordinary. Pass only the
ticket path through the existing authenticated elevated launch transport; the receiver
validates it before reading and creating recovery windows.

The window registry owns the identities of this recovery's windows, indexed by snapshot.
Retries create only missing slots; successfully created windows are not duplicated.
Failed creation is reported as failure. Acknowledgement requires every target to have
been created and each surviving target to be ready. Intentionally closed targets count
as handled, so closing them does not resurrect them on a later launch. Unrelated windows
cannot satisfy or block completion. Existing persisted recovery-attempt bounds remain.

## Rejected alternatives

- Allowing privileged sessions into normal session persistence: crosses the existing
  authority and startup boundary.
- Using the ordinary snapshot for an administrator schedule: restores the wrong workspace.
- Forwarding the new process as a normal shell launch: loses the update recovery ticket.
- Comparing the total live-window count: does not identify the windows this transaction
  was required to recover.
- Clearing a partial recovery and opening every snapshot again: duplicates already-created
  terminals and their resumed commands.

## Consequences

The existing installation prepare/commit/result protocol and session format remain the
authority. The platform adapter owns private paths; UI code owns window identities and
readiness. The new recovery module follows that responsibility boundary. Failure retains
the on-disk snapshot and bounded retry path rather than claiming successful installation
or workspace restoration.

## Validation

Existing ticket tests cover success, original-version recovery, identity mismatches,
acknowledgement and attempt bounds. The scheduled-snapshot regression distinguishes
ordinary and administrator storage, later checkpoints and missing-private-snapshot failure.
The combined ticket/scheduling test passed with both actual ordinary and elevated tokens;
its private fixture files are removed explicitly after success.
GPUI tests cover unrelated window creation/closure, missing slots, duplicate requests,
failed PTY readiness and intentional dismissal. Real helper fixtures retain cancel,
unprepared-process and failed-installation checks. Packaged product acceptance is separate.

## Supersedes

The elevated-update prohibition and the no-recovery-request limitation in the existing
administrator launch handover. Its same-user/session/elevation authentication and ordinary
Runtime isolation remain in force.

## Revisit when

Cross-process update coordination or a different workspace recovery lifecycle is required.
