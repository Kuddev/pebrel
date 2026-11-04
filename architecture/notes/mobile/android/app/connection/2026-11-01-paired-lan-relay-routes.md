# Paired LAN and relay routes

## Status

Proposed in the pull request for #840.

## Context

Android identifies a paired computer by its Noise public key. The saved profile
previously also represented its only transport: a successful LAN pairing replaced
the relay profile, and a successful relay pairing replaced the LAN profile.

## Evidence

`RelayProfile.latestByComputer` retained only the last profile for a computer, and
`SessionRepository` removed that computer's previous profile on its first snapshot.
Reconnection then used the remaining profile's single URL. The report in
[issue #840](https://github.com/Kuddev/pebrel/issues/840) exercises both paths.

## Decision

Keep one saved computer record with at most one alternate route. Only successfully
authenticated v2 routes for the same Noise host key can be combined. Each route
retains its own URL, TLS pin, relay room, bearer token and device credential.
Replacing one route preserves the other. The existing encrypted store and its
64-computer limit remain the storage authority.

The optional `alternate` object is local storage metadata. Invitation parsing and
wire serialization do not import or transmit it. Stored alternates must have the
same computer identity, the other transport mode, and a device credential rather
than an invitation. Nested alternates are rejected. Older single-route records
still load; an older client will discard the alternate on its next save.

One repository-owned connection attempt tries saved LAN first, then relay. LAN
has a five-second connection budget when a verified relay fallback exists. Each
route is attempted once before the existing foreground reconnect backoff takes
over. Certificate, authentication and protocol failures terminate the attempt.
New invitations and changed addresses are tried directly and are saved only after
authentication and a runtime snapshot, preserving the existing enrollment boundary.

The workspace ID, pane drafts and notification cursor survive route changes.
Client identity checks reject stale callbacks. Closing a workspace also discards
its pending route attempt. Disconnected library rows show both saved transports;
connected rows continue to identify the active transport.

## Rejected alternatives

- Separate computer cards per transport would split workspace identity and drafts.
- Reusing a route's credentials on another endpoint would conflate independent
  TLS pins and authorization scopes.
- Accepting alternate routes from a QR payload would persist unverified endpoints.
- Retrying LAN indefinitely would prevent using a working saved relay.
- Simultaneous connections would complicate enrollment and connection ownership.

## Consequences

Previously overwritten credentials cannot be recovered: the missing transport
must be paired again once. This change selects routes at connection/reconnection;
it does not continuously probe and replace an already healthy relay connection.
LAN address rediscovery continues to require the previously pinned certificate.

## Validation

`RelayRoutesTest` covers both pairing orders, storage round trips, route rotation,
legacy records, untrusted alternates, LAN preference, bounded fallback and fatal
errors. `DesktopRecoveryTest` covers workspace/draft retention and callback cleanup
through the real repository. `HomeSessionsTest` covers one selectable computer row
with a localized dual-route caption. The PR records executed checks separately
from Android device and UI tests that were not run locally.

## Supersedes

None.

## Revisit when

The desktop advertises an authenticated multi-endpoint invitation or the mobile
client supports more than one route of each type. Such a change must preserve
computer identity, endpoint-specific credentials and bounded connection ownership.
