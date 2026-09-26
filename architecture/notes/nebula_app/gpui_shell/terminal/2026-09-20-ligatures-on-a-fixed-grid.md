# Ligatures on a fixed terminal grid

## Status

Implemented for issue #220.

## Context

The terminal enabled `calt` on fonts but shaped every cell separately. A font
could not form a ligature from adjacent cells. The theme's existing ligature
preference was also not consumed by the GPUI renderer.

Terminal hit testing, cursor movement, selection and PTY dimensions use fixed
columns. Natural text advances cannot become the source of terminal geometry.
The previous renderer deliberately avoided GPUI's `force_width` tolerance:
small deviations could alternate between natural and snapped positions after
an edit, visibly moving unrelated characters.

## Evidence

- [`element.rs`](../../../../../nebula_app/src/gpui_shell/terminal/element.rs)
  owns viewport projection, colors, cursor and selection boundaries.
- [`ligatures.rs`](../../../../../nebula_app/src/gpui_shell/terminal/ligatures.rs)
  maps the shaper's UTF-8 cluster indices to columns for ASCII spans. Multiple
  glyphs can share an index, and a ligature can consume multiple source cells;
  glyph counts cannot be used as column counts.
- The pinned GPUI API returns shared natural layouts. Mutating a cached layout
  would also change other terminal views using it.

## Decision

The settings core owns `ligatures=on|off|theme`. Missing or invalid values and
reset use `on`, as requested for the product default. Explicit `theme` follows
the selected theme's existing boolean; built-in themes default to enabled.
The settings pane exposes a localized menu and uses the existing persistence
and global settings update path, including updates to open terminal views.

The renderer shapes contiguous, identically styled ASCII graphic cells together
when enabled. Whitespace, wide cells, combining sequences, colors, font styles,
cursor inversion, selection and formula projection delimit these spans. Other
text retains per-cell rendering. A disabled setting uses per-cell rendering
and explicitly disables `calt`, `liga` and `clig` for all four font faces.

For a shaped span, each cluster starts at its source column times cell width;
glyphs within the same cluster keep their relative offsets. The renderer makes
a private layout with the fixed span width, preserving the cached natural
layout, glyph identities, vertical positions and decorations. Neither path
passes `force_width`. Grid content, copying and cursor coordinates remain owned
by the terminal model.

## Rejected alternatives

- Merely toggling font features cannot join characters shaped in separate calls.
- Natural line layout or `force_width` would reintroduce variable positioning.
- Treating every glyph as a cell breaks fonts that substitute several glyphs
  in one cluster or one glyph for multiple characters.
- General complex-script shaping requires a separate policy for wide cells,
  combining sequences and bidi order; it is not inferred from ASCII offsets.

## Consequences

The implementation adds no dependency or thread. It scans visible cells and
copies glyph layouts only for joined spans. Shaping uses GPUI's existing line
cache; there is no additional persistent document or terminal cache. Costs are
linear in visible cells and shaped glyphs, but this is not a measured claim of
better frame time. Font-provided ligature outlines still determine their ink
bounds; proportional fonts do not become monospaced fonts.

## Validation

- Settings tests cover old/invalid configuration, all menu values, precedence,
  persistence and reset to enabled.
- Rendered settings tests use the actual menu with mouse opening, keyboard
  confirmation, cancellation, reopening and the effective global setting.
- Layout regressions cover style/overlay boundaries, combining and wide text,
  multi-glyph clusters, deletion and fractional cell widths.
- A Linux headless test uses the real platform text system and bundled font to
  compare enabled/disabled glyphs and verify cached layouts stay unchanged.
  The normal GPUI test context uses a no-op text system and cannot prove this.
- These tests do not replace visual inspection of every user-supplied font or
  Windows/macOS text-rasterization behavior.

## Supersedes

None.

## Revisit when

Supporting complex-script ligatures, changing GPUI's shaping/layout API, or
profiling demonstrates that copying visible glyph layouts is a material cost.
