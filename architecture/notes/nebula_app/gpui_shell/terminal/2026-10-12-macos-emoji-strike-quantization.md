# macOS color-emoji strike quantization breaks one-shot span fitting

## Status

Implemented on 2026-10-12 after the user reported the 2026-10-10 overwide fix
working on Windows but not on macOS.

Supersedes: none — amends
[`2026-10-10-overwide-glyph-span-fitting.md`](2026-10-10-overwide-glyph-span-fitting.md)
for color-emoji fallback glyphs only; its tolerance path is unchanged for every
other glyph.

## Context

The 2026-10-10 fix fits overwide cells by re-shaping once at
`font_size × span / natural_width`. That arithmetic assumes the fallback font's
advance scales linearly with font size, which holds for Windows Segoe UI Emoji
(≈ 1.373 em uniformly). On macOS the emoji fallback is AppleColorEmoji, a
bitmap `sbix` font whose reported advance is quantized to absolute pixel
steps, so the assumption fails twice:

1. **Detection misses.** At font sizes 10–19 px the measured advance exceeds
   the two-column span (0.6 em step ⇒ 1.2 em span) by 0.2–1.8 px, i.e. ratios
   1.009–1.14 — most sizes land *under* the wide-cell 1.1 tolerance and are
   never shrunk (default font size 11.25 pt ⇒ 15 px: advance 20 px vs span
   18 px, ratio 1.111, only barely caught).
2. **One shot cannot converge.** Where the tolerance does fire, re-shaping at
   the proportional size lands on the next quantization step, not at the span:
   at 14 px the first re-shape yields 18 px against a 16.8 px span, still
   overflowing 1.2 px into the neighbor column — the visible "emoji glued to
   the text at its right".

## Evidence

Measured on this machine (macOS 12.7.6, CoreText, bundled
MapleMonoNormal-NF-CN-Regular as the primary font) on 2026-10-12:

- Maple Mono NF CN does not cover emoji codepoints; CTLine cascades 👍/✅/🦀/⏺
  to AppleColorEmoji and reports the ink at ≈ 1 em square.
- Advance by font size (px): 10→13, 11→15, 12→16, 13→17, 14→19, 15→20,
  16→21, 17→22, 18→22, 19→23, 20→23, 24→25, 26→26 (equal from 26 up).
  Two-column span = 2 × ⌊0.6 × size × scale⌋ / scale.
- Re-shaping 15 px at ×0.9 (13.5 px) yields 18 px — converges by luck;
  re-shaping 14 px at ×0.884 (12.4 px) yields 17–18 px against span 16.8 px —
  does not converge.
- GPUI marks glyphs whose run font is AppleColorEmoji with
  `ShapedGlyph::is_emoji` on macOS (`gpui_macos` text system); the Windows
  DirectWrite backend never sets the flag.

## Decision

`shape_cell_text` branches on the shaped line containing an `is_emoji` glyph:

- Emoji fallback glyphs fit by **iterating**: shrink the font size by
  `target / measured_width`, re-shape, re-measure — until the advance fits
  the target, stops decreasing (strike floor), or a small step cap (4) is
  hit. The pure model is `fit_emoji_font_size` in
  [`cell_text.rs`](../../../../../nebula_app/src/gpui_shell/terminal/element/cell_text.rs);
  quantized strikes converge in ≤ 2 measured steps, each hitting GPUI's line
  layout cache.
- The iteration target is **not** the span itself: `emoji_fit_target`
  reserves 1/6 of the span (0.2 em for a wide slot) as trailing air. Windows
  Segoe UI Emoji gets that air for free from its ≈ 0.2 em ink right bearing;
  AppleColorEmoji ink fills its advance, so fitting to the bare span still
  glued the glyph to the right neighbor (user re-check on 2026-10-12: better
  but still tight). Emoji render ≈ 14 % under the nominal size on both
  platforms — the 2026-10-10 consequence, now symmetric.
- Everything else keeps the 2026-10-10 tolerance path untouched, so Windows
  behavior (and the CJK / PUA carve-outs) is bit-identical: `is_emoji` is
  never set there.

## Rejected alternatives

- Tightening the wide tolerance below 1.1 to catch ratios like 1.09 would also
  sweep up compact-cell CJK rounding (measured 28.8 px advance against a
  floored 28 px span at 24 px/scale 1) and re-rasterize ordinary CJK.
- Widening the grid slot for emoji — rejected for the same column-alignment
  reasons as 2026-10-10.
- Scaling at paint time (`ShapedLine::paint` / `paint_emoji`) would fork
  GPUI's pinned painting API for one caller; also rejected 2026-10-10.
- Detecting "fallback font ≠ requested font" instead of `is_emoji` would also
  catch CJK fallbacks (① via PingFang) and change their fitted behavior.

## Consequences

macOS color emoji in the grid (agent TUIs, ghost, completion popup) now fit
their two-column span minus a fixed 1/6-of-span air gap — any residual
overflow is bounded by one quantization step below visual threshold (≤ 0.2 px
when the strike floor blocks the last step). Emoji cells pay up to 4
`shape_line` calls on first appearance; subsequent frames hit the layout
cache. Windows and Linux paths are unchanged (`is_emoji` unset on Windows).

## Validation

- `emoji_strike_fitting` unit tests pin: quantized convergence, strike-floor
  termination (≤ 2 measures), linear one-step convergence, the within-span
  short circuit, and the 1/6-span trailing-air target.
- The existing `overwide_fitting` tolerance tests are unchanged and still pass.
- Full suite: `cargo test --ignore-rust-version --locked -p nebula --features
  gpui-shell` (the two environment-bound `git_completion` failures predate
  this change).

## Supersedes

None (amendment, see Status).

## Revisit when

- GPUI exposes per-glyph scaling, or the bundled font/emoji fallback changes
  its advance behavior (re-measure the step table above).
- A user reports a glyph relying on the strike-floor residual overflow.
