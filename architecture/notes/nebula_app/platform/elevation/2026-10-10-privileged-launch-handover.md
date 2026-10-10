# Elevated launches share only an elevated window owner

## Status

Implemented for review. Native compilation and focused regressions are required;
interactive UAC acceptance is recorded separately from automated tests.

## Context

Repeated administrator launches ignored `windowing_behavior`. Ordinary resident
handover excludes elevated processes and explicit `-e` commands, while the UAC
launcher intentionally passes the selected program as an explicit argument vector.
Removing either exclusion alone would lose privilege separation or launch identity.

## Evidence

- `main.rs::try_hand_over_to_resident` excludes both cases.
- `platform/elevation.rs::arguments` preserves the selected program and arguments.
- `runtime_api/server.rs` deliberately keeps privileged endpoint discovery off disk.
- Windows [named-pipe security](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)
  documents the permissive default descriptor and the native access check.
- [Mandatory Integrity Control](https://learn.microsoft.com/en-us/windows/win32/secauthz/mandatory-integrity-control)
  defines no-read-up and no-write-up independently of discretionary permissions.
- Tokio 1.50 exposes explicit security attributes and first-instance creation.
  Its named-pipe `poll_flush` is a no-op; disconnecting before the peer consumes
  a reply can discard that reply.

## Decision

Use a Windows-only, startup-only named pipe for elevated GPUI launch requests.
The first-instance flag elects one owner before tray, hooks and Runtime startup.
The scope includes the executable, data directory, explicit configuration file,
user SID and Windows session. A protected administrator/SYSTEM DACL and high
integrity no-read-up/no-write-up label replace the default pipe descriptor.

Both sides obtain the other PID from the kernel and verify its process token:
elevated, same user, same Windows session. Clients use identification-only SQOS.
After verification, the launching client grants foreground permission only to
that owner PID; an OS denial does not duplicate an already accepted launch.
The pipe carries only a bounded launch request and acknowledgement; it never
publishes a Runtime bearer token or falls back to ordinary endpoint discovery.

The receiver reuses the window registry's existing new-window/current-desktop/MRU
policy, launch conversion, tab creation and focus operations. Original program,
arguments and directory are retained. Shared startup conversion and its existing
tests live together under `windowing/startup.rs`, instead of duplicating that rule
or extending the already-full window registry module.

One owned worker sleeps on pipe I/O per elevated application, not per pane or
frame. Dropping its guard stops the task and releases its handles. Requests have
size/time bounds; disconnect or timeout cancels an event still waiting for the UI.
Claimed events execute once. Errors after transmission never start a fallback
process, since an acknowledgement failure does not prove creation was undone.
After replying, the server waits for client closure before disconnecting the pipe.

## Rejected alternatives

- Reusing `runtime.port` or an inherited endpoint: crosses the ordinary/elevated
  ownership boundary and can direct an administrator launch to a normal process.
- Publishing a privileged TCP token in the ordinary data directory: exposes a
  privileged control capability to readers of that directory.
- Replacing the selected program with the default shell: loses explicit launch
  arguments and profile behavior.
- A second window-selection policy or per-pane service: duplicates existing
  responsibilities and adds ongoing resource cost unrelated to startup.

## Consequences

Ordinary startup, explicit ordinary commands, legacy-shell startup, session
isolation and private Runtime child discovery are unchanged. No dependency,
persisted setting, public Runtime method or new UI control is added. Administrator
windows still do not restore/write the ordinary session or become tray residents.
Existing older processes without this endpoint are not retrofitted in place.

## Validation

Focused tests cover wire bounds and literal argv, cancellation/duplicate delivery,
token role/user/session checks, configured window reuse versus new windows, original
tab preservation, invalid shell identity and shutdown. The native pipe test asserts
rejection for an ordinary test process and real round trips for an elevated one.
UI fixtures use the existing settings guard and its nextest serialization group.
These are not a claim that the user's live UAC workflow has been exercised.

## Supersedes

The independent-process behavior for subsequent elevated GPUI launches in ADR-0010.
Its ordinary-session and privileged-Runtime isolation contracts remain in force.

## Revisit when

A supported platform API provides equivalent same-user/same-session privileged
handover with owned cancellation, or a separately reviewed requirement changes
the current process and persisted-session isolation boundary.
