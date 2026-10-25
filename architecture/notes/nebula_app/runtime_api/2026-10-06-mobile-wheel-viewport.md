# Mobile wheel forwarding and controlled viewport

## Status

Implemented; focused Rust and Android validation passed. Physical-device
acceptance remains separate from the integrating PR checks.

## Context

The phone's colored terminal view panned only inside its current frame. An AI
CLI never received a wheel event, so reaching the frame boundary stopped reading
instead of scrolling the conversation. The existing input path accepted only
text and keys. Reading the live grid also hid desktop scrollback movement.

## Evidence

- `TerminalSnapshotView` changes local offsets in its gesture callback; its
  input target previously exposed no wheel operation.
- `DesktopTerminalInput` owns an ordered, bounded writer; the runtime client has
  a separate dispatch allowlist which must also accept negotiated wheel input.
- `TerminalView::on_scroll` already decides mouse reporting, alternate-screen
  cursor keys, and ordinary scrollback. Reusing it preserves desktop semantics.
- The default `capture_terminal_screen` and text tail intentionally ignore the
  desktop display offset; external readers depend on that distinction.

## Decision

Add `pane.scroll` to the authenticated input policy, with explicit mobile window
and pane IDs, bounded signed line counts, and desktop grid coordinates. Advertise
the capability only for the GPUI implementation. The phone negotiates it before
dispatch and reuses the existing non-replaying writer, pending limits, and owner
checks. Input permission remains required even in composer mode. Scroll replies
contain only the action result: wheel gestures do not rebuild a full window/tab
snapshot merely to discard it at the bridge.

Pan the phone's locally clipped grid first. At its vertical boundary, accumulate
fractional remaining movement and forward whole rows. Pinch zoom and selection
retain their gesture ownership. Phone-only reflow remains local because its
cells have different desktop coordinates. Wheel input uses pixel-precise native
scrolling, without applying the desktop discrete-wheel multiplier a second time.

Add an explicit `screen_viewport` read option for a controller following desktop
scrollback. Keep the existing live-grid and text-tail defaults unchanged. Use one
optional `ScreenMode` internally so an absent screen and a viewport screen are
not represented by contradictory booleans. Preserve existing frame bounds,
palette, styles, wide glyphs, delta encoding and connection scope.

## Rejected alternatives

- Always send Page Up: different CLIs own scrolling differently, and normal
  terminal input should not receive an unrelated key sequence.
- Accumulate all received frames as history: redraws are not appended output.
- Transfer all scrollback on every update: it increases work and traffic while
  retaining the same gesture-to-application defect.
- Change every screen reader to the desktop display offset: it changes existing
  external-reader semantics without an explicit request.
- Resize the shared PTY or activate the desktop window: neither is required to
  control its scrolling, and both disturb the desktop user's workspace.

## Consequences

The feature requires matching phone and desktop support; older desktops retain
their prior local-pan behavior. Read-only connections keep local reading without
forwarding input. No new thread, dependency, unbounded cache, or retry of an
uncertain wheel event is introduced. The controller follows the shared desktop
viewport rather than maintaining a separate private scrollback position.

## Validation

Rust regressions exercise mouse reports, alternate-screen keys, normal history,
target bounds, permission checks and opt-in viewport capture. Android regressions
exercise real touch events, composer-mode scrolling, pinch isolation, ordered
dispatch, capability negotiation and viewport requests through the transport.
Physical-phone and external AI CLI acceptance remain distinct from these tests.

## Supersedes

Extends the explicit projection choice in
[mobile color grid](2026-09-28-mobile-color-grid.md); its default live-grid contract
and resource limits remain in effect.

## Revisit when

Controllers need independent history positions, or a negotiated screen protocol
adds full pointer input and a different viewport ownership model.
