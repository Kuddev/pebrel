# Overwide glyph span fitting on the fixed grid

## Status

Implemented on 2026-10-10 after the user reported emoji icons glued to the
content at their right inside agent TUIs (Claude Code).

Superseded by
[`2026-10-12-macos-emoji-strike-quantization.md`](2026-10-12-macos-emoji-strike-quantization.md)
for color-emoji fallback glyphs (macOS quantized advances now fit by
iteration); the tolerance path below is unchanged for all other glyphs.

## Context

The GPUI terminal positions every cell at `column × cell_width`; the right
neighbor always starts at its own cell origin regardless of the previous
glyph's natural advance (the module contract in `element.rs`). Wide graphemes
(emoji, East Asian text) own two columns ≈ 1.2 em with the default 0.6 em
Maple Mono NF CN step. Windows emoji fallback (Segoe UI Emoji) has a uniform
1.373 em advance, so a color emoji bleeds ~0.17 em into the neighbor column
and its ink visually touches the next character.

The legacy GL renderer had already solved the narrow variant of this class:
East Asian Ambiguous characters (①) fall back to full-width CJK glyphs inside
a one-column slot and were scaled into one column at glyph-load time
(`renderer/text/glyph_cache.rs`, 2026-07-28), with Private Use Area icons
exempted so Nerd Font prompt badges keep their full ink.

## Evidence

- Maple Mono NF CN: ASCII advance 600/1000 em, CJK 1200/1000 em — a proper
  wide glyph exactly fills its two-column span, so overflow detection can be
  tight for wide cells.
- Segoe UI Emoji: advance 2812/2048 em ≈ 1.373 em for sampled emoji; ink
  right bearing ≈ 0.2 em, so fitting the advance to the span still leaves
  visible air before the neighbor.
- [`cell_text.rs`](../../../../../nebula_app/src/gpui_shell/terminal/element/cell_text.rs)
  owns the decision; [`element.rs`](../../../../../nebula_app/src/gpui_shell/terminal/element.rs)
  passes each cell's span (1 or 2 × cell_width) and wide flag.

## Decision

`shape_cell_text` shapes a cell's grapheme, compares the shaped width with
the cell's column span, and when it overflows beyond tolerance re-shapes at
`font_size × span / natural_width`. Re-rasterization happens on the CPU in
GPUI's text-system cache; the GPU never stretches. The smaller line is
vertically centered by `ShapedLine::paint` within the row's line height.

Tolerances: wide cells 1.1× span (proper full-width glyphs measure exactly
the span; compact cell-width rounding stays below the threshold so ordinary
CJK is never re-rasterized), narrow cells 1.4× span — the legacy numbers that
spare italic overhang while catching full-width fallbacks like ①. The first
character being Private Use Area disables shrinking entirely, preserving the
legacy prompt-badge carve-out. Ghost/completion overlay text goes through the
same helper via `grid_text`.

## Rejected alternatives

- Widening the grid slot for emoji changes column geometry the application
  already laid out (Claude Code aligns its TUI by columns); rejected for the
  same reason as the legacy ① fix.
- `force_width` repositions glyph origins but neither scales ink nor applies
  per cell; it also carries the 1 px snap tolerance the grid contract avoids.
- Shrinking at rasterization time inside the text system would fork GPUI's
  pinned shaping API for one caller.
- Shrinking every overwide glyph without tolerance would re-rasterize all CJK
  under floored compact cell widths and slightly blur it.

## Consequences

Overwide cells pay one extra `shape_line` call only when they actually
overflow; per-cell shaping already dominates this path. Emoji render ~14%
smaller than their nominal font size, matching how the legacy shell fits
① into one column and other terminals fit emoji into two. The legacy
`legacy-shell` renderer keeps its load-time bitmap scaling and does not
share this code; its narrow-slot behavior is the precedent, not a dependency.

## Validation

- Unit tests in `cell_text.rs` pin the wide tolerance (emoji shrinks, CJK at
  the span and slightly above it does not), the narrow 1.4 legacy tolerance,
  and the PUA carve-out.
- The full workspace/terminal test suites pass; the two `git_completion`
  failures reproduce on the pristine tree and are environment-bound.
- Shaping itself is exercised by the existing Linux headless real-font tests;
  Windows test contexts use a no-op text system and cannot prove raster sizes.

## Supersedes

None.

## Revisit when

The bundled font changes its advance (re-measure the 0.6/1.2/1.373 em
constants cited above), GPUI exposes per-glyph scaling, or a user reports
that a non-PUA glyph they rely on got visibly smaller.
