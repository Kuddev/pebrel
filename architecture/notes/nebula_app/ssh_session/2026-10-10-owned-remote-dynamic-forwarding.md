# Pane-owned remote and dynamic SSH forwarding

## Status

Proposed for review with #552. Validation outcomes are recorded in the PR.

## Context

The Ports dialog previously supported only local forwarding. Remote forwarding
introduces server-initiated channels and a remote listener that survives dropping
a request future unless explicitly cancelled. Dynamic forwarding introduces an
untrusted local protocol handshake before a direct-tcpip channel can be opened.

Looking up a destination in the connection pool again also cannot guarantee that
a forward uses its pane's connection after the saved profile or route changes.

## Evidence

The pinned russh 0.62.2 client handler accepts forwarded-tcpip channels by default.
Its tcpip-forward API sends a global request and awaits a reply; cancellation of
that await is not a server-side cancel-tcpip-forward request. See the pinned
[handler contract](https://docs.rs/russh/0.62.2/russh/client/trait.Handler.html) and
[`forward/remote.rs`](../../../../nebula_app/src/ssh_session/forward/remote.rs).

SOCKS5 negotiation and CONNECT address forms follow
[RFC 1928](https://www.rfc-editor.org/rfc/rfc1928.html). This capability supports
TCP CONNECT only, including hostname resolution on the SSH host.

## Decision

The shell lifecycle supplies a weak Forwarder for its exact authenticated
connection. It is invalidated with the pane's shell scope. Ports never resolves a
destination, reads credentials or authenticates a replacement connection.
Established forwards also watch that scope, so cleanup does not depend on the UI
processing its exit event first. Stale results cannot install an inactive forward.

Each authenticated connection owns one reverse-forward authorization table, shared
with its handler. A pane-owned forward exclusively reserves a nonzero remote port
before sending its request. Only matching loopback address/port channels with a
live owner are dispatched; unregistered, stale and excess channels are rejected.
The table is not process-global and same-host connections cannot share entries.

A remote worker owns the registration request through its bounded reply wait even
if its caller disappears. It consumes authorized channels concurrently with that
wait: RFC 4254 does not require the global reply to precede forwarded channels.
On stopping or refusal it disables routing, drops queued replies, aborts all
copies and sends cancellation. Only a confirmed cancel releases the reservation;
refusal or timeout leaves a disabled tombstone until that transport ends. An old
server listener therefore cannot acquire a new local target through port reuse.

Local listeners bind IPv4 loopback. Remote requests bind server IPv4 loopback and
connect only to a local loopback target. Dynamic listeners offer no-auth SOCKS5
only on local loopback; BIND, UDP ASSOCIATE and password methods are unavailable.
The handshake has fixed-size address buffers and one five-second budget covering
negotiation and channel creation. Each listener admits at most 64 copies; remote
permits include queued channels as well as active copies.

The dialog starts in Local mode, displays direction/type for each rule, validates
only the local port in Dynamic mode, and stops rules by identity rather than port
number. No saved schema, new dependency or independent SSH process is introduced.
New messages have English/Simplified Chinese entries and explicit English fallback
for the other existing catalogs.

## Rejected alternatives

- Destination-based reauthentication can silently replace a pane's connection.
- A global address-to-target map confuses different connections and pane owners.
- Dropping an in-flight request or aborting its worker can orphan a remote bind.
- Accepting every server channel exposes arbitrary local targets without a rule.
- An unbounded accept/handshake loop permits unlimited copy buffers and tasks.
- SOCKS UDP/BIND and wildcard binds require distinct authorization/lifetime policy.

## Consequences

Forward creation/stopping preserves the authenticated transport used by the shell
and other forwards. Reverse registration cleanup may finish after the owner has
closed, within the existing network budgets. The remote SSH server must honor the
requested bind address; its forwarding policy can refuse registration/cancellation.

## Validation

`forward/tests.rs` and `forward/tests/modes.rs` use synthetic trusted host keys,
actual russh request/channel exchange and owned loopback sockets. They cover
mixed modes, byte/half-close behavior, exact transport ownership, scope closing,
unauthorized channels, conflicts/refusal, cancellation before the global reply,
pending application confirmation, bounded concurrency, SOCKS address/command
rejection and handshake expiry. The pending-confirmation fixture gates result
delivery after a real wire acknowledgment; it does not simulate packet encryption.
Real cancellation refusal and the existing 20-second cancellation timeout retain
the tombstone; a late wire ACK does not reauthorize it. Confirmed cancel permits reuse.
The GPUI Ports tests exercise real controls, input geometry, Tab traversal,
Space/Enter down/up events and mode validation. A gated synthetic result exercises
the real task/completion path across source-window closure before destination draw.
These fixtures do not use user profiles, credentials or a production SSH server.

## Supersedes

The remote/dynamic exclusion in the 2026-09-26 pane-local-forwarding note.
Its loopback, ownership and bounded-concurrency rationale remains applicable.

## Revisit when

Persistent rules, reconnecting forwards, wildcard binds or SOCKS BIND/UDP are
explicitly requested, or the pinned SSH provider's request cancellation contract changes.
