# Explicit independent GUI launches

## Status

Proposed in the implementation PR for issue #348.

## Context

Ordinary launches hand their directory and shell to the resident process. Issue
#348 requests a way to start another process. Skipping only the initial handover
is insufficient: GPUI also hands over after losing the runtime ownership lock.
Continuing without a runtime server would leave that process's panes without
their own control endpoint, while session checkpoints could overwrite the
resident's saved workspace.

## Evidence

[`main::try_hand_over_to_resident`](../../../../../nebula_app/src/main.rs) and
[`gpui_shell::run_shell`](../../../../../nebula_app/src/gpui_shell/mod.rs) implement
the two handover paths.
[`RuntimeServer::spawn_at`](../../../../../nebula_app/src/runtime_api/server.rs)
already supports an unpublished endpoint and exports it to terminal children.
[`SessionPersistence`](../../../../../nebula_app/src/gpui_shell/workspace/session_persistence.rs)
already supports in-memory snapshots for isolated windows. Both facilities previously selected
that scope only through the elevation policy.

## Decision

Record the explicit `--new-instance` intent once after CLI parsing. Startup
ownership uses one predicate combining that intent with the existing elevation
requirement. The explicit option is limited to GPUI launches.

The process starts a fresh terminal, owns an unpublished runtime endpoint, and
keeps automatic workspace snapshots in memory. Its children receive that
endpoint even when the process was launched from another Pebrel terminal.
Closing its last window exits instead of hiding an undiscoverable resident.
Ordinary launches retain their current handover, discovery, and session policy.

The explicit launch also bypasses elevated handover and automatic restoration
or installation of a previously scheduled update. Saved mobile connections are
not automatically resumed by the extra instance. Settings and other user data
remain shared; this option is not a separate configuration profile.

## Rejected alternatives

- Skip handover but keep shared discovery: the resident lock still prevents a
  runtime server, and clients can target the wrong process.
- Overwrite `runtime.port`: redirects existing clients and competes with the
  resident's cleanup.
- Export an inherited environment flag: every nested invocation would silently
  acquire the same launch policy instead of requiring explicit intent.
- Create a temporary settings tree: changes configuration and persistence
  semantics beyond the requested process separation.

## Consequences

Independent windows do not participate in ordinary session restoration. Users
can still explicitly export a workspace. No dependencies, protocol fields, or
new persisted settings are introduced. The startup flag is immutable for the
process lifetime and does not alter privilege detection.

## Validation

[CLI regression coverage](../../../../../nebula_app/src/cli.rs) checks terminal
launch arguments. The [independent-instance tests](../../../../../nebula_app/src/gpui_shell/workspace/windowing/new_instance_tests.rs)
check incompatible launch modes. A subprocess test uses the real startup
predicate and runtime constructor while the resident discovery lock is held,
verifies the child endpoint with a PING, and checks shared discovery and session
bytes after checkpoints, close, quit, and server drop. This is process and socket evidence, not native desktop
visual acceptance.

## Supersedes

None.

## Revisit when

Independent instances need persistent profiles or per-instance update recovery;
those need explicit storage ownership rather than shared-file overwrites.
