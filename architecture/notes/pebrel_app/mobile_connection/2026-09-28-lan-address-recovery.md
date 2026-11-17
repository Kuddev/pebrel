# Preserve paired computer identity across LAN address changes

## Status

Implemented in the working tree. Native TLS and Android recovery tests are verified;
desktop restart and emulator network acceptance are recorded separately.

## Context

A saved LAN address can disappear after changing networks. The settings selector
can show a current interface even when startup failed to bind the saved address.
Previously, selecting the new address generated a new TLS key, room and bearer
token. An already approved phone therefore lost its transport credentials even
though the desktop still retained its device grant.

## Evidence

- `mobile_connection.rs::start_route` previously regenerated `LanCredentials`
  whenever the address or requested port changed.
- `preview/lan.rs` stores the TLS private key, certificate, room and transport token
  independently of `endpoint/host.rs`'s Noise identity and per-phone grants.
- `endpoint/discovery.rs` already advertises the endpoint and its SPKI fingerprint.
- Android's `PinnedDesktopTls` pins SPKI and retains normal SAN/hostname validation.
  Simply changing a URL while retaining the old IP certificate does not work.
- [rcgen 0.14.10](https://docs.rs/rcgen/0.14.10/rcgen/struct.CertificateParams.html#method.self_signed)
  supports issuing a new certificate with an existing signing key.
- [Android NSD](https://developer.android.com/develop/connectivity/wifi/use-nsd)
  requires discovery to be stopped when its owning lifecycle no longer needs it.

## Decision

Relocate LAN credentials by issuing a certificate for the new IP using the existing
private key. Preserve SPKI, room, transport token and device authorization. Retain
an automatically selected port on restart; explicit port changes remain explicit.

Use the existing process-level startup thread as a five-second LAN recovery owner.
Only enabled LAN routes enumerate interfaces, off the UI thread. Keep the selected
address while it exists; otherwise choose from the existing ordered interface
list. Missing or failed listeners are retried without requiring settings to remain
open. No probe packets or new external dependency are introduced.

Recovery tries the existing serialized operation lock. It reads preferences under
that ownership, never advances the user-operation generation and uses the same
transaction/rollback path. A concurrent user operation can cancel recovery rather
than having old preferences overwrite the user's selection.

On Android, reuse NSD only while a foreground, previously connected LAN workspace
needs recovery. Match the already saved SPKI, not the advertised name. Discovery
can change only the endpoint: TLS pin, room, bearer token, Noise host and grant
remain the saved values. TLS, Noise and the first valid Runtime snapshot must
succeed before replacing the saved address. Reuse the computer ID and draft;
never replay commands. Backgrounding, closing the workspace or recovering all
connections stops discovery. Identity failures still stop automatic retries.

Saved-computer and connected-workspace menus also share an explicit address editor
(`ComputerAddressDialog`). `RelayProfile::atLanAddress` validates the address and
retains all saved identities and credentials. It enters the same connection path
as discovery: only a successful authenticated snapshot replaces the saved address.
This allows recovery on networks where multicast discovery does not arrive.

## Rejected alternatives

- Regenerating transport credentials on every network change: turns an address
  change into another pairing and invalidates approved phones.
- Disabling hostname verification: hides an invalid certificate rather than
  renewing its SAN with the already trusted key.
- Trusting an mDNS name or newly advertised pin: untrusted local metadata is not
  permission to replace an existing computer identity.
- Refreshing only from the settings page: recovery would depend on a view lifetime.
- Persisting an address as soon as it is discovered: an unreachable or spoofed
  announcement could replace the last authenticated connection.

## Consequences

Multicast reachability still depends on the network and emulator. This does not
add LAN/relay racing or cross-network discovery. Older desktop builds that already
discarded their previous LAN key do not regain that key through this change.
The process owns one background monitor; no per-view network service is added.

## Validation

`preview::lan::tests` verifies real TLS on a new interface, unchanged credentials,
updated SAN, listener restart and rejection of wrong credentials/old epochs.
Android `RelayConnectionTest`, `DesktopRecoveryTest` and `DesktopReconnectTest`
verify endpoint selection, rejected pins/invitations, save-after-snapshot,
workspace/draft retention, stale callbacks, background disposal and no input replay.
The address-editor changes passed 47 focused Android tests across
`RelayConnectionTest`, `DesktopRecoveryTest` and `HomeSessionsTest`.
Emulator acceptance used the real address-editor entry, rejected an invalid input,
then connected at the changed LAN address without scanning or granting access again.
The saved list retained one entry per existing desktop identity. Two later isolated
desktop restarts also recovered the open phone connection at the same endpoint.
These checks do not establish automatic mDNS address discovery in the emulator or
physical-phone multicast reachability.

## Supersedes

Extends `2026-09-26-process-owned-phone-settings.md` with LAN recovery ownership.
Supersedes the pairing-form-only discovery scope in
`architecture/notes/mobile/link/2026-09-26-native-phone-pairing.md`, decision 6;
the enrollment and cryptographic boundaries in that note remain unchanged.

## Revisit when

LAN/relay route racing, an OS network-change subscription, or recovery without
multicast becomes an explicit requirement. Preserve the existing identity and
authentication-before-persistence boundaries.
