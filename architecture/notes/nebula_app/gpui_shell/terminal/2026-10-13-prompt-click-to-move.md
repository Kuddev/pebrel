# Move the prompt cursor with a plain click

## Status

Implemented for review.

## Context

Users expect editor-style cursor placement at a shell prompt: click
inside the command being typed and the shell cursor moves there. The terminal
cannot set the shell's line-editor cursor directly; only the shell owns it.

## Evidence

The terminal core already tracks OSC 133;A/B through `nebula_prompt_active` and
`nebula_prompt_input_point`, so the start of the editable input is known while a
prompt is idle. `write_input` keeps the native prompt alive only for plain CSI
cursor keys (`event_mailbox::preserves_native_prompt`); SS3 application-cursor
arrows end it. zsh ZLE, bash readline and PSReadLine bind CSI arrows by default.

## Decision

`Term::nebula_prompt_cursor_offset` returns the signed character count between
the cursor and a clicked grid point inside the current prompt input. It counts
cells while skipping wide-character spacers, follows soft wraps of the cursor's
logical line, and clamps clicks past the typed text to just after it. Points
before the input start or outside that logical line return `None`.

The GPUI pointer path calls it only for a plain left click that went through the
ordinary selection path and left no selection, while the `click_to_move_cursor`
setting (Interaction → Mouse and selection, on by default) is enabled. It clears
the typed-line mirror and ghost suggestion as keyboard arrows do, then writes
that many CSI Left or Right arrows through `write_input`, never through
broadcast input.

## Rejected alternatives

- Query the shell's line buffer through the completion editor protocol first: it
  is asynchronous and only available for shells with Pebrel's editor reporting.
- Use mode-dependent arrow encoding: SS3 arrows end the native prompt state.
- Broadcast the arrows: other panes have different input geometry.
- Move the cursor on any click on the screen: outside the input, arrows would
  edit shell history or reach unrelated programs.

## Consequences

The feature works only where the shell reports OSC 133;B. Without it, clicks
keep their existing selection-only behavior. A right-side prompt on the same row
counts as text, so a click past the input may send extra Right arrows; line
editors stop at the end of the buffer. Explicit multi-line input rows separated
by hard newlines are not targeted.

## Validation

`prompt_click_offsets_count_characters_inside_the_logical_input` covers wide
characters, soft wraps, clicks past the text and points outside the input.
`plain_click_moves_the_prompt_cursor_but_selection_gestures_do_not` covers the
real GPUI mouse path, Shift+click, drag selection, the cleared suggestion and the
disabled setting. `click_to_move_cursor_setting_is_searchable_and_has_a_visible_switch`
covers the settings control.

## Supersedes

None.

## Revisit when

Pebrel gains a synchronous line-editor protocol that can set the cursor directly,
or shells commonly report multi-line input boundaries.
