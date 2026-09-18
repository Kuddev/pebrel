# Native mobile link foundation

This crate implements native pairing and the independently deployed protocol-v2
relay. Desktop Settings and the Android 0.4.4 source now have v2 adapters; this is
not a claim of physical-phone or real-VPS acceptance. The shipped 0.4.3 phone uses
**v1** and cannot use a v2 server. Native LAN keeps an explicit v1 compatibility
adapter; no v2 failure ever selects it automatically.

## Implemented boundaries

- Shared Noise NKpsk0/25519/ChaChaPoly/SHA256 channels through `snow 0.10.0`.
  Host static identity is pinned out of band. A per-invitation/per-device PSK
  authenticates the controller; relay bearer tokens are a separate credential.
- New ephemeral keys per connection, ordered transport nonces, fail-closed
  replay/reordering/tamper handling, bounded fragmentation and reassembly.
- Host-owned expiring, one-use invitations; approval replaces the invitation
  with a different per-device credential. Revocation requires the host owner to
  cancel that device's live connection as well as remove its grant.
- Native QR matrix/RGBA generation, including a four-module quiet zone. No Node,
  browser, image generation service or external QR endpoint.
- A TLS WebSocket relay binary. No application payload logging or decryption,
  database, offline replay, user account system, Docker or Node dependency.
- Linux systemd 247+ and Alpine/OpenRC install/start/stop/status/uninstall backend. systemd supplies
  an unprivileged DynamicUser and read-only credentials. Port binding capability
  is isolated to the service. The installer does not modify firewall rules or
  existing web servers. Keep-configuration uninstall and later reinstall work;
  purge removes only unmodified files listed in the ownership manifest.
  Stable progress events describe actual stages, not estimated percentages.
  Service-manager commands have deadlines; start waits for bounded readiness
  probes rather than assuming process creation means the listener is ready.
  A readiness failure can be retried without generating replacement access keys.
  On OpenRC, the existing supervise-daemon owns restart. The listener loads keys
  and binds once, drops root to nobody before starting network threads, and fails
  closed if that privilege drop is unavailable. No account or package is installed.
- Android JNI uses this same Rust channel implementation; it does not duplicate
  cryptographic framing in Kotlin. The app adapter validates versioned invitations,
  persists approved profiles only after Runtime validation and a first snapshot,
  and retains a rotated credential in memory for a retry after Runtime failure.
- Native desktop v2 endpoints pin the server SPKI from verified bootstrap, perform
  Noise enrollment and persist host/device secrets through the OS credential store.
  A bounded Runtime actor is created only after an encrypted enrollment acknowledgement.
  The relay configuration never receives the host key or the device E2EE secret.
  Credential-store size limits can reject additional device enrollment; there is
  no fallback to plaintext storage.

## Transport contract

`GET /v2/link?device=<room>&role=desktop|mobile` requires a role-specific bearer
token in the Authorization header. Configuration stores SHA256 hashes, never
those raw tokens. `/healthz` indicates liveness; `/readyz` also checks admission
and shutdown state. Cleartext listeners are allowed only on loopback for a
separately managed TLS reverse proxy or isolated local tests.

Control frames are `relay.waiting` and `relay.paired` JSON with `version: 2`.
The latter carries an opaque fresh link epoch. Both endpoints bind the version,
host/grant identity and epoch in the Noise prologue. Application traffic uses
binary frames only, at most 65,535 bytes each; the maximum reassembled message
is 2 MiB. The encrypted fragment header is big-endian total length and offset
(two u32 values). The endpoints, not the relay, authenticate those headers.

Each socket has at most eight queued packets. Slow peers, malformed traffic,
heartbeat loss or peer departure close the link; the other socket cannot be
reassigned to a new peer. Reconnection must establish new keys and resubscribe.
Never automatically replay a possibly delivered terminal command.

## Development and deployment boundary

Build the independent executable with:

```sh
cargo build --locked -p pebrel-mobile-link --features relay --bin pebrel-relay
```

`init` requires a new empty, absolute directory and generates a TLS certificate,
separate role credentials and configuration. `export-access` explicitly exports
sensitive connection bootstrap information; it must travel over verified SSH and
must never enter progress logs or a public artifact.

The service installer expects a locally staged executable and a SHA256 obtained
from a separately verified release manifest. The executable/manifest delivery UI
and update/rollback transaction are not implemented yet. Installation is not
reported successful before the local authenticated TLS readiness check; the UI
must additionally verify the public endpoint before claiming remote reachability.

Desktop Settings includes native LAN address selection and QR, plus explicit v1
or v2 relay configuration import. Consumed/expired v2 invitations are hidden.
Desktop relay reconnection establishes a new Noise channel; phone reconnection is
currently the explicit retry action. Automatic LAN/relay racing, live revocation
UI, and the SSH one-click install/update/uninstall card remain integration work.
Do not silently downgrade v2 to v1 if any step fails.

## Verification

Run only this component's tests with:

```sh
cargo test --locked -p pebrel-mobile-link --features preview
```

Tests cover real loopback WebSockets/TLS, channel authentication, bounded queues,
invitation lifecycle and installer-owned fixture files. Service-manager calls in
filesystem tests are injected: these tests do not certify a real systemd/VPS
installation, Android JNI on a device, or the final GPUI pairing workflow.
