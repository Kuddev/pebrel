# Cursor blinking presentation

## Status
Implemented.

## Context
The existing cursor timer only toggled visibility. Smooth fading, breathing and
vertical expansion need presentation frames without changing terminal geometry.

## Evidence
[`TerminalView`](../../../../../nebula_app/src/gpui_shell/terminal/view.rs) already owns an epoch-cancelled cursor timer, focus/IME subscriptions
and input-triggered restarts. `cursor_painter.rs` supplies the same cursor bounds
to ordinary rendering, glyph inversion and the optional shader effect frame.

## Decision
`nebula_settings::CursorBlinkStyle` owns `blink`, `smooth`, `phase`, `expand`, and
`solid` under `cursor_blink_style`. An explicit valid named style wins over the
legacy boolean. Missing/invalid styles honor `cursor_blink=0` as `solid`; otherwise
the default is `blink`. The effective boolean remains available to old consumers.
Reset removes both preferences.

The GPUI presentation adapter samples opacity/vertical scale over a 1500ms cycle.
`blink` retains the 750ms binary timer; animated modes use 16ms ticks. Typing,
style changes and focus changes restart visibly. Epoch cancellation remains owned
by each terminal entity. Hidden output, unfocused windows/panes, IME preedit and
VT-hidden/steady cursors stop scheduling. `solid` suppresses even VT requests to
blink. Reduced motion substitutes binary blinking for the three animated modes.

Painting scales the cursor about its vertical center, fades glyph inversion with
the block, and feeds the same rectangle/color into shader cursor uniforms. The
terminal snapshot, input/IME anchor, grid and movement trajectory are unchanged.

## Rejected alternatives
A global animation timer would keep hidden panes alive. Animating the terminal
cursor coordinates would move input/IME anchors. Treating `solid` as an animation
with an always-visible frame would leave unnecessary wakeups.

## Consequences
At most the visible focused pane runs a smooth blink timer. Steady cursors and
applications that hide the VT cursor retain their protocol semantics.

## Validation
[Preference compatibility/round trips/reset](../../../../../nebula_settings/src/cursor_blink.rs); [waveform](../../../../../nebula_app/src/gpui_shell/terminal/cursor_blink.rs) and shape geometry tests;
rendered settings choices, reopen/cancel and existing focus/IME/motion regressions;
hot application preserves the session and logical cursor, including VT requests.

## Supersedes
None.

## Revisit when
Blinks need user-controlled timing or the rendering engine exposes a shared
animation clock that preserves these lifecycle gates.
