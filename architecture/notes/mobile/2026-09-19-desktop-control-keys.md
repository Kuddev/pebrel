# Mobile desktop control keys

## Status

Accepted for the Android connection preview.

## Context

The Android command composer already exposes Esc, Tab, Ctrl+C and arrow keys for
local terminals. The desktop Runtime already validates the same restricted intent
through `pane.send_key`, and the mobile bridge allowlist includes that method, but
the Android desktop screen did not route those controls. Treating the broad
`input` flag as proof of key support would make an older desktop fail at runtime.

## Evidence

- `RuntimeCommand::from_request` rejects arbitrary key bytes and printable letter
  keys without Control, leaving `pane.prompt` as the text-input authority.
- Both SSH and relay transports return a `mobile.ready` capability object before
  Android enables input.
- Prompt delivery can be uncertain after a transport timeout or disconnect, so a
  control key must follow the same no-replay rule.

## Decision

Advertise a separate `send_keys` capability only for input-authorized channels.
Android maps the seven existing composer labels to the Runtime's named-key schema,
including an explicit Control modifier for Ctrl+C. Prompt and control-key requests
share a per-connection mutex so taps cannot overtake a submitted command. A result
is never retried automatically. Desktops that omit `send_keys` retain prompt input
but do not expose the remote control-key button.

## Rejected alternatives

- Send escape sequences or control bytes through `pane.prompt`: this bypasses the
  terminal-mode-aware Runtime contract and broadens the injection surface.
- Infer key support from `input=true`: older bridges may support prompts without
  implementing the named-key route.
- Retry after timeout or reconnect: the first Ctrl+C or command may already have
  reached the terminal, so replay could interrupt or execute the wrong task.

## Consequences

SSH and self-hosted relay connections expose the same negotiated controls. Input
remains shared with the computer rather than an exclusive takeover. The current
desktop view is still a bounded plain-text tail; this change does not claim live
colour-grid streaming or speculative local echo.

## Validation

Android unit tests cover every label and legacy-capability fallback. Relay tests
exercise real forwarding to the loopback Runtime fixture, and Rust tests bind the
SSH capability to explicit input authorization. CI still cannot establish physical
device latency or acceptance for rapid repeated key taps.

## Supersedes

None.

## Revisit when

The protocol gains an exclusive input lease, structured terminal grid streaming,
or a versioned key catalog shared directly with the desktop Runtime.
