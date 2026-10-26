# Bounded character-art regions without changing text rows

## Status

Local preview implementation. Keep the approved 2.2.2 text spacing; desktop
acceptance of the combined text and character-art scene precedes integration.

## Context

Restoring normal reading space also restored the tall aspect ratio of multi-row
character art. A global 1:2 grid had corrected logos but made paragraphs and lists
too tight. Ordinary text spacing must no longer be used to fix a graphic.

## Evidence

Braille uses two dot columns and four dot rows. Its source grid may have taller
rows, but geometric glyphs can use a local two-times-column-width pitch. Merely
changing individual dot positions leaves gaps between terminal rows; the complete
region and its input coordinates must share the same mapping.

The ordinary render snapshot contains only source rows in the viewport. Centered
compression can leave pixels from a source row just outside that viewport visible.
Reusing only cached bounds would drop those pixels early and mis-map selection.

## Decision

- The frontend identifies bounded, multi-row regions made exclusively from dense
  Braille, block characters, or long repeated slash runs. It does not inspect
  process names. Short decorations, ordinary prose, table borders, Powerline,
  explicit backgrounds, images and formula/native-result coverage are excluded.
- Draw a qualifying graphic with local row pitch equal to twice the existing
  column width, centered in its original reserved rectangle. The terminal grid,
  text font size, PTY dimensions and surrounding rows do not change.
- Selection backgrounds, search highlights, link decoration and pointer inversion
  use the same local mapping. Cursor-intersecting regions and VI mode retain grid
  presentation. Graphic slash runs do not use programming ligature coalescing.
- Retain only bounded source-region coordinates, not stale glyphs. A partially
  visible known region requests its current neighboring source rows through the
  shared `RenderSnapshot::capture_from` pipeline under the existing terminal lock.
  This preserves colors and selection without duplicating snapshot semantics or
  changing the actual viewport. Negative relative source rows remain addressable.
- Recompute complete boundaries from current source data. Erasing or shrinking a
  region must yield the same layout as a newly created view. Reset, screen switch
  and column changes invalidate the coordinate cache. The candidate scan and cache
  have explicit bounds; insufficient evidence leaves the existing grid visible.

## Rejected alternatives

- Change default text row height again: contradicts the accepted reading result.
- Scale only dots inside each cell: leaves character-boundary gaps.
- Move graphic pixels without input mapping: selection targets the wrong row.
- Cache full rendered glyphs: can display stale output after an offscreen rewrite.
- Treat every punctuation or Braille run as a picture: changes ordinary text.
- Claim arbitrary ASCII art recognition: text and ASCII drawings can be ambiguous;
  this slice covers the observed Braille and repeated-slash cases.

## Consequences

The graphic keeps its original vertical allocation, including its surrounding
space; following output does not jump. Partially observed regions without known
complete boundaries use normal grid rendering. Source text and copy semantics
remain authoritative. No setting, application-specific hook or dependency is added.

## Validation

Focused regressions cover excluded text and borders, neighboring system-info text,
selection inversion, actual drag-selection copying, source-row capture, partial
scrolling, erase/shrink history independence and reset. Native captures compare the
approved reading scene pixel-for-pixel and measure a circle while retaining the
29-device-pixel Maple 13px row at 150% display scale. Fastfetch uses its actual
Windows 11 output; a live Codex preview remains separate from synthetic geometry.

## Supersedes

None. The default typography contract from
[2.2.2 reading spacing](2026-10-26-restore-2-2-2-reading-spacing.md) remains unchanged.

## Revisit when

A real false-positive or interaction case demonstrates ambiguity, or applications
provide an explicit image/graphic region protocol that replaces shape detection.
