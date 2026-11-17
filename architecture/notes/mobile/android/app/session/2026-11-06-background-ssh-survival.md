# Keeping background SSH tabs connected

## Status

Accepted by the maintainer on 2026-11-06, who reported SSH tabs dropping soon after
the app went to the background.

## Context

Two paths closed working connections. A new default network finished every SSH
tab at once, although a VPN change, Wi-Fi roaming or a second SIM often leaves the
old route working. And without a wake lock the CPU sleeps, so keepalives stop and
servers or the client's own keepalive limit end the session.

## Evidence

Code inspection of `networkChanged` and `SessionService`. The existing recovery
tests still pass: a probe that fails reconnects in the background, and closing a
tab cancels its probe. No physical-device battery or Doze measurement was made.

## Decision

On a network change, a ready SSH tab is probed with a round trip and reconnects
only if the probe fails or times out (5 s). The session service holds its partial
wake lock by default while sessions live; the notification action still releases it,
and that choice is kept until the user takes it again.

## Rejected alternatives

- Keep reconnecting every tab on any network change: it discards sessions that
  were fine, including running programs without a multiplexer.
- Raise the native keepalive limits: it needs a native rebuild and does not help
  while the CPU sleeps.

## Consequences

Battery use rises while sessions are open in the background; the notification
shows that the wake lock is held. A dead route now takes up to 5 s longer to
reconnect.

## Validation

`SshRecoveryTest` passes.

## Supersedes

None.

## Revisit when

Device measurements show the wake lock costs too much, or Android restricts it
for foreground services of this type.
