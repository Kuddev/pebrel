# Character-art grid and Braille spacing

## Status

Implemented; native Windows/macOS visual acceptance remains outstanding.

Default row spacing is superseded by [natural text rows and symbol fallback](2026-10-25-restore-text-metrics-and-symbol-fallback.md); Braille geometry is retained.

## Context

Terminal character art uses row/column geometry, not an independently scaled
image. A tall grid distorts both ASCII/block logos and Braille dot graphics.
Braille also needs continuous dot pitch across character boundaries.

## Evidence

The bundled Maple font has a 0.6em advance and 1.32em hhea height. The previous
Windows default added four device pixels. At 14 logical pixels and 150% DPI,
Compact mode produced a 12 × 31 device-pixel cell. Removing the offset alone
would still produce 12 × 27. The font's Braille ink occupies only part of that
height, so using its outlines adds a large gap between rows of dots.

## Decision

- The GPUI terminal defaults to 1.2em rows and no implicit platform offset.
  This pairs with the bundled 0.6em advance to form a 1:2 grid before rounding.
  Font size, glyph outlines, shaped baseline metrics and width modes are unchanged.
  Explicit theme line height and device-pixel offsets still override the default.
- Route U+2800–U+28FF through the existing core geometry contract. Dot centers
  subdivide each cell into 2 × 4 equal regions, including across row boundaries.
  Dots remain circular under custom spacing. Fractional centers are antialiased;
  individually snapping centers would introduce uneven spacing at odd cell sizes.
- The frontend paints circles through its existing quad API. Grid text, selection,
  copy, color resolution, cursor placement and PTY sizing retain their owners.

## Rejected alternatives

- Rescale Codex/Fastfetch images: these are character cells, and other programs
  require the same correction.
- Only remove Windows padding: this leaves both residual distortion and Braille gaps.
- Change bundled font files or shrink text: this affects unrelated typography and
  does not establish a renderer-independent Braille pitch.

## Consequences

Default rows become shorter and a fixed-size viewport can contain more rows.
Startup sizing and prepaint use the same typography function. Custom fonts with
unusual vertical extents can still choose an explicit theme line height; this
default does not promise a 1:2 ratio for every possible advance width.
Ordinary text is not clipped per cell or resized to fit the new spacing.
Braille emits up to eight circle quads and one primitive-vector allocation per
occupied cell; the blank pattern allocates no primitives or font-shaping work.

## Validation

Core geometry tests cover all 256 patterns, neighboring code points, odd cell
sizes, scaling and cross-cell pitch. Snapshot tests cover colors, selection,
cursor coordinates and copying. GPUI tests cover defaults and explicit overrides.
An opt-in native fixture exercises a 65 × 65 dot circle, full dot grids, CJK,
accents, box/block joins and Powerline through TerminalElement.

Linux ARM64/Xvfb software-rendered captures at 100%/150%/200% produced circle
bounds 258 × 258, 387 × 403 and 516 × 532 device pixels. Dot-row gaps were 4,
6–7 and 8–9 pixels respectively, without character-boundary gaps. The native
fixture verifies startup/prepaint metrics and consumes actual resize messages.
Its 60 forced draws had median CPU time about 26.6ms per scale in the unoptimized
CI profile. This is fixture cost evidence, not an optimized product benchmark
or a comparison with the previous renderer.

## Supersedes

None.

## Revisit when

Another terminal backend adopts this grid policy, a font exposes a demonstrable
clipping regression, or native captures show a mismatch in geometry or dot pitch.
