# Independent glyphs and proportional terminal grids

## Status

Default typography is superseded by [the 2.2.2 reading-spacing restoration](2026-10-26-restore-2-2-2-reading-spacing.md).

Implemented and inspected on native Windows at 150% scaling. Other native
platforms remain covered by their selected CI jobs, not this desktop inspection.

## Context

Using font ascent plus descent and platform padding as the row height made a
Maple 14px grid 12 by 31 device pixels. Braille dots occupied equal subdivisions
of each cell, but their vertical pitch was 29% larger than their horizontal
pitch. Restoring that spacing preserved the character-art distortion.

## Evidence

The locked GPUI ShapedLine paints glyphs at its original font size. Its row
height only supplies the centered baseline. It does not scale outlines to fit
the row. Separate width and height rounding also left residual distortion in
the earlier 0.6em-width/1.2em-height policy.

`view/typography.rs` owns cell measurement and baseline placement, while
`element.rs` supplies the main face's baseline to text painting. Existing
`nebula_terminal::render::boxdraw` primitives own special-character geometry.

## Decision

- Shape ordinary text at the configured font size. Use the main face's ascent
  and descent to center its baseline; align styled and fallback runs to that
  baseline by translating the paint origin. Do not change shaped glyph sizes,
  advances or outlines, and do not introduce a per-row clip.
- Measure and round the configured column width once. Default row height is
  twice that device-pixel width, so the two columns and four rows of a Braille
  cell have identical pitch. Recover the integer device width before doubling
  it to avoid floating-point floor errors at fractional display scales.
- Keep theme row multipliers and explicit device-pixel offsets authoritative.
  Default platform offsets are zero. Explicit spacing can intentionally produce
  a different aspect ratio.
- Retain geometry-rendered Braille, box drawing, blocks and Powerline. Fonts,
  text baseline placement and geometry are separate responsibilities on the
  same grid used by selection, cursor positioning and PTY size reporting.

## Rejected alternatives

- Restore natural font height: measured to reproduce the 29% distortion.
- Independently round 1.2 times font size: leaves unequal pitch after column
  rounding, especially at fractional display scaling.
- Shrink glyphs into rows or widen their outlines: changes ordinary text.
- Move only the dots within a tall row: introduces gaps at character boundaries.
- Detect a particular application or logo: unrelated terminal programs need the
  same grid, and application detection cannot define text geometry.

## Consequences

Changing the primary font may change column advance and the corresponding row
height. Natural vertical font metrics do not expand the grid. The default glyph
size stays unchanged; users with unusually tall glyphs can still choose an
explicit row multiplier. No saved setting, dependency or terminal protocol is
added. Baseline alignment adds constant arithmetic per shaped text run, with
no new allocation or font shaping.

## Validation

Existing grid tests now cover six display scales, six font sizes, four advances
and both width modes, including floating-point round trips. Baseline tests
verify that the shared layout and glyph size remain unchanged. Explicit theme
row multipliers and offsets retain their tests.

Native Windows A/B with Maple 14px at 150% scaling measured a Braille ring of
387 by 499 pixels under restored natural rows, and 387 by 387 under the default
proportional grid. A separate text-only capture measured identical 370 by 27
pixel ink bounds. Shaping probes also retained font size, advance and font IDs.
Maple 13px and Consolas 14px both produced 355 by 355 pixel rings. The existing
60-frame synthetic fixture measured median draw CPU time of 4.73ms for Maple
14px versus 4.29ms with taller rows; the shorter grid contains more visible rows,
so these values are local fixture evidence, not an isolated overhead benchmark.

Fastfetch 2.70.0 ran through the real GPUI product's Windows PTY with a Windows
11 logo and system information, returned zero, and was inspected in the rendered
window. No distribution package or non-Windows desktop result is implied.

## Supersedes

The natural-row default in [text metrics and symbol fallback](2026-10-25-restore-text-metrics-and-symbol-fallback.md)
and independent 1.2em row rounding in [character-grid spacing](2026-10-25-character-grid-spacing.md).
Their symbol-font ownership and special-character geometry remain unchanged.

## Revisit when

A representative font demonstrates glyph overlap under the default proportional
grid, or a native platform produces unequal device-pixel pitch despite the
shared measurement contract.
