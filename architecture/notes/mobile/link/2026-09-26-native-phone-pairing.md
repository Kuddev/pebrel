# Native phone pairing and short-code discovery

## Status

Implemented in the working tree. Native protocol tests and product compilation are
verified; physical-phone network acceptance remains separate.

## Context

Phone Remote needs an actual Off → Pairing → Paired lifecycle, not a settings-only
QR display. The desktop remains the execution authority. LAN and a separately
hosted relay must share device grants and input permission without sharing relay
credentials with the runtime API.

Manual pairing needs both a complete invitation and an eight-digit code. A short
decimal code has insufficient entropy to replace a device credential or Noise PSK.

## Evidence

- `mobile/link/src/endpoint/host.rs` owns enrollment, persistence-before-ack,
  per-device permission and revocation.
- `mobile/link/src/endpoint/transport.rs` requires Noise authentication, desktop
  approval and an encrypted credential acknowledgment before opening Runtime.
- `mobile/link/src/endpoint/pairing_code.rs` limits a short code to five attempts,
  one redemption and the original invitation's validity.
- `mobile/link/src/endpoint/discovery.rs` advertises a v2 TLS endpoint, name and
  public-key fingerprint, never the invitation or device secret.
- Android's NSD service-info callback is available from API 34. API 26–33 resolve
  services one at a time; concurrent legacy resolution can return ALREADY_ACTIVE.
- The runtime snapshot subscription publishes state changes, not an unconditional
  polling heartbeat. Permission UI therefore needs a separate live update.

## Decision

1. Keep pairing and Noise framing in the shared Rust link crate. Kotlin uses its
   existing JNI bridge, and GPUI calls the process-owned desktop service.
2. Use 600-second one-use invitations and a bounded 120-second approval window.
   Rejecting or replacing an invitation also cancels its outstanding approvals.
3. An eight-digit code retrieves the full invitation over pinned TLS on the local
   network. It does not grant access. mDNS fingerprints are untrusted bootstrap
   metadata: the user compares the locally derived six-digit Noise code on both
   devices and approves on the desktop to bind the intended computer and phone.
4. Retain QR and complete-invitation import when multicast discovery is blocked or
   when pairing across networks. Discovery is optional; the native TLS service is
   not torn down if advertisement registration fails.
5. Persist host grants before returning the new per-device secret. Revoke or change
   permissions only after credential-store success. A watch notification updates
   the phone's input UI even while the terminal is idle; every runtime request
   still checks the current authorization independently.
6. Keep Android discovery and lookup scoped to the pairing form. Closing the form
   stops discovery and cancels its socket; waiting for desktop approval remains a
   live foreground session rather than triggering reconnect or service shutdown.
7. Pause disconnects listeners and sessions but retains enrolled devices. Settings
   view disposal does not stop approved sessions.

## Rejected alternatives

- Deriving the Noise PSK from eight digits: enables offline guessing and merges
  bootstrap usability with the long-term authorization boundary.
- Advertising invitation secrets in mDNS: exposes enrollment material to everyone
  on the subnet without an explicit user pairing action.
- Automatically enrolling after scanning: the QR alone must not open Runtime.
- Reimplementing Noise in Kotlin or treating a server-supplied code as verification:
  both create a second authority for cryptographic state.
- Waiting for a new terminal snapshot before updating permissions: leaves an idle
  phone showing writable input after permission has changed.
- Automatic v2-to-v1 fallback: silently removes v2 enrollment properties.

## Consequences

Multicast discovery depends on local network policy. Manual full invitations remain
available. A discovered fingerprint alone is not a verified computer identity.
LAN and relay endpoints can coexist, with an existing LAN session taking priority
for the same device. Automatic phone-side route racing/failover is not implemented.
The relay currently admits one mobile socket per room; native LAN admits multiple
enrolled phones. OS credential-store size limits can reject additional enrollment
without storing secrets in plaintext.

## Validation

Focused link tests exercise real loopback TLS, code redemption and rejection,
expiry/attempt limits, Noise code agreement, approval, credential persistence,
permission changes, refresh, pause and revocation. Android tests use TLS mock
servers and the actual client adapter; they do not replace a physical-phone test.
GPUI layout and clipboard checks are separate from protocol and compilation checks.

## Supersedes

None.

## Revisit when

A verified cross-network short-code service, multi-phone relay rooms or automatic
transport switching becomes an explicit requirement; preserve the same enrollment
and per-device authorization boundaries when adding it.
