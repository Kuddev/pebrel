# Natural text rows and independent symbol fallback

## Status

Row spacing is superseded by [independent glyphs and proportional grids](2026-10-25-independent-glyphs-and-proportional-grids.md). Symbol font ownership remains in force.

Implemented; desktop visual and memory acceptance remain separate from compile
and focused regression checks.

## Context

Using 1.2em rows and removing the Windows offset improved character-art geometry
but changed the density of ordinary terminal text. The requested rollback must
restore the previous spacing without discarding the independent Braille geometry.
Powerline and interface icons also implicitly required the complete CJK font,
including when another terminal text font had been selected.

## Evidence

The prepaint and startup paths share `view/typography.rs`. GPUI passes font size
to glyph rasterization separately from row height. Row height positions the
baseline and grid; it is not a vertical glyph scale. A taller grid can therefore
stretch multi-row character art without stretching any individual text outline.

The complete bundled Maple face is 20,563,672 bytes. Its renamed symbol subset is
1,941,228 bytes, preserves private-use icon advances, and has no Latin/CJK text
coverage. Font bytes are not equivalent to resident process memory. The pinned
Windows backend already retains a font owner to avoid copying the complete bytes.

## Decision

- Default rows again use shaped ascent plus descent and the original platform
  offset, including four device pixels on Windows. Explicit theme row multipliers
  and offsets retain their existing authority and rounding semantics.
- Keep continuous Braille geometry and built-in Powerline separators. Their
  positions use the actual grid. Arbitrary user row spacing cannot guarantee a
  1:2 cell ratio or a circular multi-cell graphic.
- Register the symbol face at startup. Register complete Maple only when selected
  by a terminal/style/CJK/interface font chain or a theme editor draft. Preserve
  explicit fallback order without applying the draft to global settings.
- Without a separate CJK override, use the main text font and platform fallback.
  UI icons use the symbol face; UI monospace metadata uses system monospace.
- Registration belongs to the application lifecycle and occurs during settings
  application or terminal creation, before shaping. Native caches retain a font
  once loaded; switching away does not pretend to unload it.

## Rejected alternatives

- Change font size or scale text to improve a logo: changes unrelated reading.
- Delete the complete font: removes an explicitly retained user choice.
- Copy only currently visible icons: breaks other user prompt themes.
- Claim the font-file delta as an RSS saving: native caches and mapped pages are
  separate quantities requiring measurement.

## Consequences

The previous text density returns. A fresh process using another text font can
avoid registering the complete Maple face. Existing configurations explicitly
selecting Maple still load it. No persistent setting names or terminal protocols
change. The independent font preserves OFL licensing and uses a new family name.

## Validation

Focused checks cover row metrics and explicit offsets, symbol glyph coverage and
advances, configured fallback order and UI font persistence. Windows desktop
inspection at 150% scaling with Maple at 14 logical pixels measured an 8 by 20.67
logical-pixel cell: text and joins render, while the Braille circle remains taller
with restored spacing. Font regeneration reproduces the checked-in bytes.
Process-memory measurement remains separate; no unmeasured RSS reduction is claimed.

## Supersedes

The default row-height/platform-offset decision in
[character-grid-spacing](2026-10-25-character-grid-spacing.md). Its continuous
Braille geometry remains in force.

## Revisit when

A separately reviewed font/grid policy replaces the restored default, or native
font lifetime APIs permit safe unloading without invalidating live glyph caches.
