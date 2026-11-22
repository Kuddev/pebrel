# Keep LAN changes independent of relay availability

## Status

Implemented; focused validation is recorded below. Installed desktop and physical-phone
acceptance remain separate from source and isolated transport checks.

## Context

A computer can connect directly to a phone that provides its Wi-Fi hotspot. If an
old virtual adapter remains selected, choosing the hotspot adapter must commit the
new LAN listener independently of an enabled relay that is offline.

## Evidence

`mobile_connection::apply_locked` previously rebuilt every enabled route before
committing any of them. A failed relay connection dropped a newly created LAN
candidate, restored the old configuration and returned a generic connection error.
The settings selector retained the attempted address even after that rollback.

A local fixture loading the previous production module reproduced the failure:
an enabled relay on a closed TCP port caused LAN startup to return `Connection`
and left no LAN invitation. The fixture uses in-memory credentials, separate ports
and the actual transport implementation; it does not modify installed settings.

## Decision

Pass the affected route through the existing settings transaction. Adapter, port
and LAN-toggle operations rebuild only LAN; relay validation and configuration
operations rebuild only relay. Selecting a pairing route requests only that route.
Global operations keep their existing all-route transaction semantics.

LAN recovery scopes its transaction to LAN. Startup restores enabled routes in
separate transactions so a later relay failure retains an already committed LAN
listener. Credential persistence, operation serialization, generation cancellation
and rollback remain owned by the existing application module.

After a failed settings transaction, reset the adapter selector to the address in
the committed snapshot. Keep the error visible instead of displaying the attempted
address as if it had taken effect.

## Rejected alternatives

- Automatically disable or erase relay settings: an offline server does not revoke
  the user's preference or its credentials.
- Bind every adapter: expands the listening surface and ignores explicit adapter
  selection.
- Replace the TLS identity or relax certificate checks: neither fixes transaction
  coupling; existing SAN renewal and key preservation remain in effect.
- Introduce a new connection manager: the existing transaction already owns the
  necessary state, so a route parameter is sufficient for this incident.

## Consequences

A LAN-only operation does not retry an unrelated failed relay. Relay configuration
still reports its own connection errors. This does not add route racing, automatic
network switching or a new persisted format.

## Validation

- Previous production code reproduced LAN rollback when the configured relay
  refused TCP connections.
- The focused GPUI regression exercises the failed-operation callback and checks
  that the selected address returns to the committed snapshot.
- The same isolated fixture with modified production files verified adapter
  replacement, persisted address, a generated invitation and unchanged LAN
  identities while the relay was offline. A subsequent explicit relay failure and
  a cancelled operation both retained the listener. Pause removed it normally.
- The GPUI product and its test targets passed `cargo check` on Windows. The new
  rendered-control regression has been compiled; its native execution is pending.
- The phone's separate TLS error still requires a new authenticated connection;
  local route validation alone does not establish its cause.

## Supersedes

Refines route transaction scope in `2026-09-28-lan-address-recovery.md`; identity,
certificate validation and explicit adapter selection contracts remain unchanged.

## Revisit when

Global preference transactions or relay recovery acquire a requirement for partial
success reporting. Preserve explicit failure state and per-route lifecycle ownership.
