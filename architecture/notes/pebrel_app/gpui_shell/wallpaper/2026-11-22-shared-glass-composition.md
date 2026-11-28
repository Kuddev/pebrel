# Shared wallpaper composition for dark glass

## Status

Implemented for the 2.6.2 test build.

## Context

The dark static wallpaper was painted below separate gray chrome and terminal
surfaces. A second terminal-only image reveal made the titlebar and body diverge.
The combined alpha remained below one even when window opacity was one.
Settings retained the earlier 18 px blur and two 54 percent panel overlays.

## Evidence

An opaque fixture passed through the original strip filter produced corner alpha
73/255 and edge alpha 136/255. The native 2.6.2 window also changed color when the
window behind it was minimized. The selected HTML uses a shared backdrop and
S1 Mica with a broader blur, increased saturation and a single theme tint.

## Decision

Dark static extended wallpapers with inherited tab material use one window base
and one fitted image. Chrome and terminal default backgrounds become transparent;
the base owns window opacity and the image retains independent wallpaper opacity.
Custom tab material, animated wallpaper and light themes keep their composition.
Viewport edge pixels extend into filter padding while actual letterbox margins
and source transparency remain transparent.

Settings owns a black base, a cached Mica image and one theme tint. Content is
transparent and its sidebar adds a 28 percent tint. Mica uses a 56 px kernel at
1240 px reference width, saturation 1.75 and exposure 0.78. Its displayed exposure
is capped at 0.56. The original light-settings image remains available.

The existing serialized loader prepares Mica off the UI thread, with the same
generation cancellation and retirement as other wallpaper textures. Its longest
edge is capped at 512 pixels, adding at most 1 MiB per loaded wallpaper. Slider
and theme changes reuse the cache; no decode or filter runs in render callbacks.

## Rejected alternatives

- Adding more panel tint preserves the uneven layers and hides wallpaper color.
- Replacing the light-settings texture changes a separate, already supported look.
- Fitting a full Mica texture per window adds geometry-triggered work and lifetime
  state. A source-sized bounded cache is sufficient for this test build.

## Consequences

Settings blur scales with the fitted image, so extreme window aspect ratios do
not reproduce the HTML's fixed screen-space kernel exactly. Retained texture
memory increases by at most 1 MiB; transparent images remain alpha-correct.

## Validation

The isolated production loader and optics suite passes 12 tests, including
multi-scale opaque edges, narrow viewports, contain margins, transparent source
pixels, blur spread, output budgets, cancellation and cache reuse. Restoring the
old padding causes the new opaque-edge regression test to fail. The complete
application test binary passes 24 wallpaper-related tests on Linux x64.
Native screenshots cover 38, 70 and 100 percent wallpaper opacity, a white source,
light settings and no wallpaper. Minimizing the browser behind the white-source
window changes neither its top edge, left edge nor body (maximum RGB delta zero).

## Supersedes

The inherited dark workspace composition and settings-cache decision in
[precomputed wallpaper](2026-11-21-workspace-optics.md). The strip cache
ownership and resource budgets remain applicable.

## Revisit when

Settings needs an exact screen-space blur at extreme aspect ratios, or animated
backgrounds need the same fitted glass treatment.
