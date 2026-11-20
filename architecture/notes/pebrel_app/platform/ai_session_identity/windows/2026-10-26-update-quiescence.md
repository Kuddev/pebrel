# Quiesce the owned identity worker before Windows update preparation

## Status

Implemented and merged in [#775](https://github.com/Kuddev/pebrel/pull/775); native validation passed on 2026-10-26.

## Context

Windows update preparation rejects any live process from the installed executable
that is absent from the handoff's participant list. The list intentionally contains
only the application authorizing the update. The reusable identity worker is a
second copy of that executable and survives idle time until its owner exits.
Preparation happens before that exit, so a resident worker blocks the update.

The reporter runs 2.2.2, whose disposable probe can also overlap the same process
check. Their exact interfering process has not been identified. Replacing the
downloaded installer cannot modify the old updater's pre-installation check;
exiting the old application and manually running setup remains the recovery path.

## Evidence

- `worker::query_slot` retains a healthy child after returning its response.
- `handoff::prepare` writes only the current PID and creation time as participants.
- `handoff.ps1::Check-UnpreparedProcesses` rejects that second live executable
  with `Another process from this installation is not prepared for update`.
- The native regression exercises that exact production PowerShell function on a
  real worker, both before and after acquiring the update pause.

## Decision

The identity capability exposes a scoped update pause. On Windows, acquisition
closes a process-wide, reference-counted gate before waiting for the existing
worker mutex. Probes check the gate before and after locking. Acquisition reaps
only the owned worker through its kernel handle and confirms process exit before
returning. Both mutex acquisition and process cleanup remain bounded.

The update transaction acquires its pause on the background executor before
starting the native handoff helper. `PreparedUpdate` owns it during preparation,
snapshot validation and commit. Errors and cancellation drop the pause, permitting
the next poll to create a worker lazily. A committed handoff retains the gate until
process exit even when `PreparedUpdate` is dropped. Dropping a pause does not lock
or wait, since cancellation can happen on the UI thread. Other platforms have no
self-spawned resident identity worker and retain their existing behavior.

## Rejected alternatives

- Ignore helpers by name or command line: weakens the executable identity guard
  and leaves a running copy of the installed executable during replacement.
- Kill matching process names: may terminate another application instance and its
  unsaved workspace. The worker owner already has the exact native handle.
- Stop only the current worker without gating new probes: polling can immediately
  recreate it during the asynchronous preparation handshake.
- Resume probing when a committed guard drops: the shutdown path can drop its
  local handoff before process teardown finishes.
- Add the worker as an update participant: gives a probe process transaction
  responsibilities without solving its ownership and exit ordering.

## Consequences

Identity polling pauses only during an active update handoff. Existing collected
identities remain with their pane owners. A cancelled attempt can resume probing;
overlapping attempts cannot release each other's pauses. The installer still
rejects unrelated live instances and unverified process identities. No persisted
format, helper protocol, installation guard or process-killing authority changes.

## Validation

The native regression covers the original guard failure, confirmed worker exit,
concurrent polls while paused, cancellation/restart, overlapping pause owners,
bounded acquisition failure, in-flight lock ordering and commit/drop lifetime.
It also verifies that another live copy still blocks preparation and is not killed.
The existing worker failure, owner-exit and installer handoff fixtures remain.
Native Windows x64 and ARM64 execution passed, including the production
PowerShell guard regression. The full five-platform run and release workspace
checks passed on the reviewed head in [run 40264935810](https://github.com/Kuddev/pebrel/actions/runs/40264935810).
Local Linux compilation alone does not establish those Windows process contracts.

## Supersedes

The idle-worker lifetime in
[the reusable worker decision](2026-10-23-reusable-probe-process.md) is extended
with update preparation as an explicit quiescence boundary. Its process ownership,
private pipes, probe deadline and startup-feedback policy remain applicable.

## Revisit when

The application introduces authenticated cross-process update coordination or
the identity probe no longer runs a copy of the installed executable.
