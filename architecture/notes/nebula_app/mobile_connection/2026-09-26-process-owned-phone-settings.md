# Process-owned phone service and configuration transactions

## Status

Implemented in the working tree; product compilation is verified. Runtime UI and
platform-specific credential failure acceptance remain separate checks.

## Context

The settings page can be closed while a phone is connected. Its UI lifetime must
not own listeners, approved device grants or background configuration writes.
Changing one connection route must not restart the unchanged route. A cancelled
settings operation must not replace a newer user choice.

## Evidence

- `nebula_app/src/mobile_connection.rs` owns LAN/relay handles, serialized changes
  and the generation used to reject stale operations.
- `mobile_connection/preferences.rs` stores one configuration credential, keeping
  preferences and route credentials in the same OS-store transaction.
- `gpui_shell/settings_pane/mobile.rs` renders prepared snapshots, owns only UI
  tasks/feedback and cancels its outstanding configuration generation on drop.
- `runtime_api/mobile_bridge.rs` reuses the existing runtime endpoint and request
  allowlist instead of exposing the desktop endpoint token to a phone.

## Decision

Use a process-owned manager and two-worker network runtime. Run I/O and credential
operations outside rendering callbacks. UI cancellation increments an atomic
generation rather than waiting for a network or credential-store lock.

Start changed routes as inactive candidates. Commit the complete configuration
once, check the generation, then enable their Runtime scope. Failure restores the
previous configuration and only the displaced routes. Restoration failure remains
visible as a failed/missing connection; it is not reported as successful rollback.

Scope credentials to the configuration directory and elevation boundary. A disabled
startup loads preferences without creating a new host identity. Preserve pairing
credentials across pause and process restarts, but not across unrelated isolated
configuration instances.

## Rejected alternatives

- Owning listeners in the settings view: closing settings would disconnect phones.
- Saving preferences and credentials independently: partial writes leave the UI
  pointing at an old relay token or a different listener certificate.
- Cancelling by taking the manager lock in a UI callback: a slow credential write
  can freeze rendering and delay cancellation.
- Restarting both routes for every preference change: interrupts unrelated active
  phones when merely changing notification or default-permission settings.

## Consequences

Snapshot queries clone active handles before querying the host. A slow host lock
does not extend a manager-held lock into the UI. The existing credential store
remains the secret authority and storage errors remain actionable failures.
Current runtime capabilities are advertised truthfully: the restored bridge does
not claim screen-delta reads while the resident runtime only supports text reads.

## Validation

Native link tests cover persistence-before-grant, failed permission writes,
revocation and pause behavior. Product compilation covers the GPUI/service
boundary. Full platform-store rollback and real settings interaction require
their own acceptance evidence; protocol tests do not establish those results.

## Supersedes

None.

## Revisit when

The existing runtime gains native screen reads or the configuration credential
outgrows the platform's blob limit; migrate explicitly without plaintext fallback.
