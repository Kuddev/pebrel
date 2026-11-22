# Restore 2.2.2 terminal reading spacing

## Status

Implemented from the `v2.2.2` source and verified with focused regressions and
native Windows reading captures at 150% scaling.

## Context

The proportional-grid change made character-art circles round, but its default
row height also tightened ordinary multiline text. Measuring unchanged glyph ink
and a circular logo did not establish that paragraphs and lists remained readable.
The requested correction is to restore 2.2.2 typography rather than introduce
another character-art-driven default.

## Evidence

`v2.2.2` resolves to `9db57b6e794ebed88431e9e4aaef519b58986a0e`.
Its `view/typography.rs` measures the main font's ascent plus descent, applies
an explicit theme multiplier when present, then adds the device-pixel offset
and floors the result. Its Windows default offset is four device pixels.
Each shaped line is painted at its grid origin using GPUI's own baseline rule.

## Decision

Restore that typography implementation and its `line_height_for_view` adapter.
Restore the platform default offset through the existing platform identity API.
Remove the proportional-grid override and per-run shared-baseline translation.
Retain font size, width modes, explicit line-height settings and explicit offsets.

The independent symbol font, Maple registration lifetime, UI font setting and
native-result viewing changes are separate features and remain in place.
The interface font input displays the existing default, Geist, while an absent
override stays absent in persistence; clearing the input returns to Geist.

## Rejected alternatives

- Keep the 1:2 grid and change the requested font size: changes glyphs instead
  of restoring reading space.
- Apply another guessed line-height multiplier: does not restore the specified
  released behavior.
- Declare a text-only ink-size comparison sufficient: it does not measure the
  interline space that prompted this rollback.

## Consequences

Default ordinary-text spacing matches 2.2.2 again. Character-art aspect ratio
once again follows that row spacing; this rollback does not claim both a taller
row and unchanged column width still form a 1:2 grid. No application-specific
rendering heuristic or new saved preference is added.

## Validation

The typography module is compared directly with the released source. Existing
regressions cover natural metrics, DPI, explicit overrides, startup sizing,
and UI-font input save/reset. Native captures use consecutive Chinese/English
list rows, rather than treating a character-art circle as the reading oracle.
Maple 13px and 14px measured 29 and 31 device-pixel rows respectively, matching
2.2.2. A native settings capture shows Geist in the input. Sixteen focused tests
passed, including the displayed default, custom font save and empty-input reset.

## Supersedes

The default grid and shared-baseline decisions in
[independent glyphs and proportional grids](2026-10-25-independent-glyphs-and-proportional-grids.md).

## Revisit when

A separately approved spacing design has multiline reading acceptance in addition
to character-art checks; a glyph-size or logo-only check is insufficient.
