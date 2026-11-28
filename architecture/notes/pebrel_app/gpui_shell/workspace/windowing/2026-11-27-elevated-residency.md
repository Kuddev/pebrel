# Elevated workspace residency

## Status
Implemented for 2.6.2; native release validation pending.

## Context
Issue #1082 enables keep_session but closing an elevated window terminates every pane.

## Evidence
The close handler rejected all elevated instances before reading keep_session. The application already owns an authenticated same-user, same-session elevated launch channel and tray restoration.

## Decision
Allow regular elevated windows to use the existing hide operation. Keep public Runtime discovery and ordinary-session persistence isolated. Explicit independent instances on main retain their close behavior. A plain launch request carries activate_only so a hidden elevated workspace can be revealed without adding a terminal; explicit command/directory launches retain their launch semantics.

## Rejected alternatives
Removing all private-instance restrictions would mix elevated and ordinary workspace state. Merely disabling the settings switch leaves the requested background session unavailable.

## Consequences
The optional handover field is omitted when false. Old receivers reject unknown true fields rather than silently acknowledging unsupported intent. The existing tray and same-identity handover restore the resident window.

## Validation
Existing handover and hold regressions exercise request serialization and pane isolation. Release validation must also check native hide/reveal.

## Supersedes
None.

## Revisit when
Elevated handover or explicit-instance ownership changes.
