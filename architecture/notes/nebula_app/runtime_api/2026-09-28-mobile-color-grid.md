# Mobile terminal grid and phone-only reflow

## Status
Implemented; targeted protocol and Android rendering tests run locally.

## Context
The phone already had a grid decoder and painter, but `pane.read` rejected the
`screen` parameter. Plain text discarded colors and cursor information. Mirroring
desktop columns also made phone readers shrink text to see complete long rows.

## Evidence
- `runtime_api/terminal_read.rs` owns bounded terminal projections.
- `runtime_api/mobile_screen.rs` owns each link's acknowledged row baseline.
- Android `DesktopScreenFrame.kt`, `DesktopScreenSync.kt`, `TerminalReflow.kt`,
  and `TerminalSnapshotView.kt` consume the same immutable cell data.

## Decision
- Keep the existing text-tail contract unchanged. `screen=true` is additive;
  callers not requesting it receive the previous response shape.
- Advertise `pane.read.screen.v1`. Negotiate row deltas only when the resident
  runtime advertises this capability; do not advertise an unimplemented stream.
- Read the live grid independently of desktop scrollback offset. Preserve the
  theme/OSC palette, wide and combining glyphs, styles, inverse colors and cursor.
- Limit snapshots to 400 columns, 200 rows, 40,000 cells and 128 KiB of glyphs.
  Oversized snapshots return an explicit error rather than silently cropping.
- Carry optional per-row soft-wrap metadata. Phone reflow joins those physical
  rows and wraps cells to the available width without changing desktop PTY size.
- Reflow is cached by immutable source frame and available column count, not
  repeated in the paint callback. A menu switch retains the original grid view.

## Rejected alternatives
- Replaying captured ANSI: unnecessary terminal-control interpretation on a
  read-only projection and poor snapshot/delta recovery semantics.
- Resizing the shared desktop PTY to phone width: changes the computer's layout
  and affects another user's terminal interaction.
- Treating all rows as hard lines: loses logical continuity at desktop wraps.

## Consequences
Very wide fixed-layout TUIs may be better viewed with phone reflow disabled.
Old screen producers without wrap metadata can still wrap physical rows locally.
Blank grid space below a cursor must not push the actual prompt above the viewport.

## Validation
Targeted Rust screen tests cover soft/hard wraps, styles, palette, Unicode, cursor,
limits and opt-in behavior. Android tests cover delta revisions, CJK/combining/
emoji reflow, cursor placement, unchanged source grids, keyboard height and clipping.
Color output and phone-width wrapping were inspected in the LDPlayer preview APK.
This does not claim physical-phone haptic or all-device IME acceptance.

## Supersedes
None.

## Revisit when
A negotiated event-driven grid stream replaces the current bounded polling path,
or a richer screen protocol needs explicit history and viewport ownership.
