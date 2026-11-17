# Keep LAN recovery on the selected interface

## Status

Implemented; four recovery tests and two GPUI settings tests passed on Windows.
Physical Tailscale reconnect acceptance is still required.

## Context

A phone connected through a Tailscale address could stop reconnecting after the
desktop temporarily lost its Tailscale interface, while relay recovery still worked.
The required behavior is to wait for that same interface, without switching to a
Wi-Fi or Ethernet address, including temporarily.

## Evidence

`recovery::repair_lan` selected another available interface and passed that address
back to `apply_locked`, which persisted it in `Preferences.address`. Once the
fallback became the saved preference, restoring Tailscale did not select it again.
Startup performed the same normalization.

[Tailscale's address documentation](https://tailscale.com/docs/concepts/ip-and-dns-addresses)
describes node addresses as stable across physical network changes. Temporary
interface absence is not evidence that the user's selection is obsolete.

## Decision

When an address has been selected, recovery only uses that address. If it is absent,
keep the preference and credentials, stop the old LAN listener and wait. The
existing process monitor rebuilds the listener when the selected address returns.
Only a connection with no selected address chooses from the ordered interface list.
An explicit user selection remains the way to switch interfaces.

The settings selector uses the same address-selection rule. Refreshing an interface
list must not select its first entry when the saved interface is missing; enabling
LAN or reopening pairing preserves the saved address when no item is selected.

Startup and settings transactions can leave LAN pending while independently starting
or reusing relay. Missing LAN availability must not erase the preference or make
relay recovery depend on the Tailscale interface. No persistent field is added.

## Rejected alternatives

- Temporary fallback followed by switching back: violates the requirement that
  recovery stay on the same interface and changes the endpoint reachable by phones.
- Always preferring Tailscale over user choices: would silently switch explicitly
  selected non-Tailscale connections.
- Keeping the old listener when the interface disappears: its status may still
  look healthy after the interface returns, preventing a clean listener rebuild.
- Regenerating pairing credentials: existing relocation preserves the trusted key,
  room, token and phone grants even when a listener must be recreated.

## Consequences

An unavailable selected address leaves LAN waiting until it returns or the user
explicitly changes it. This deliberately replaces the old automatic DHCP/address
fallback. The existing background interval, operation lock, generation handling and
transaction rollback remain. Relay identity is unchanged.
Preferences already overwritten by an older version cannot be reconstructed;
the user must select the desired address once.

## Validation

The existing recovery tests now assert that a missing selected interface never
falls back. New cases cover IPv4 and IPv6 Tailscale disappearance, persistence
round trips during an outage and return, plus first selection and explicit changes.
The GPUI selector regression verifies disappearance, an empty list and return using
the real select component. The native screenshot test remains ignored.
Physical phone/desktop Tailscale interruption has not been exercised here.

## Supersedes

Replaces the automatic address fallback in `2026-09-28-lan-address-recovery.md`;
retains its credential and authorization rules.

## Revisit when

An explicit automatic-interface mode or same-interface DHCP tracking is required.
Never infer permission to switch interfaces from a temporary disconnection.
