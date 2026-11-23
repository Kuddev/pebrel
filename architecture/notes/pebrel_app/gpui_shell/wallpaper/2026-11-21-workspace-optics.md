# Precomputed wallpaper at the workspace sidebar

## Status

Accepted for dark workspace themes with the wallpaper extended into the chrome.

## Context

The selected L4-B prototype gives the left rail a softer image while preserving
the image detail to its right. The wallpaper slider also needs to preview the
image beneath the Settings surface and reach the full image in the terminal.

## Evidence

The approved prototype blends a 5.5 px kernel and a 2.6 px kernel into the sharp
image across a 64 px feather beyond the sidebar. Its strongest band reduces
contrast by 24 percent, then applies a theme tint and fine black-and-white grain.

## Decision

Sample the already decoded wallpaper into a window-local strip after fitting
and alignment. The file loader and strip baker share a serial preparation slot.
Each window retains one strip and limits it to 2 MiB of pixels. A short delay
coalesces geometry changes, and generation checks cancel outdated work and
prevent stale results from reaching the window.

The existing 38 percent default and the Settings background retain their
composition. Above that default, a second draw of the fitted wallpaper reveals
the image over the terminal surface so the slider can reach the original at
100 percent. Holding the wallpaper slider temporarily fades the Settings chrome
and previews the same extended image contribution. Slider movements only change
opacity and use the cached texture.

## Rejected alternatives

- Filtering each frame would repeat the same work during scrolling and dragging.
- Retaining full-window variants would duplicate unaffected pixels in every window.
- Blurring the source before fitting would make the apparent kernel size depend
  on image dimensions and the selected fit mode.

## Consequences

A changed source, fit, theme tint or settled viewport needs a new strip. The
sharp wallpaper is available while the strip is prepared. The CPU path and the
finished texture use the same BGRA and premultiplied filtering rules for
transparent edges as the existing wallpaper loader.

## Validation

The isolated loader and optics suite passed nine tests. On Linux ARM64, a
24-sample synthetic fixture measured 57–139 ms median preparation time with
0.81–2.00 MiB of retained strip pixels across 720p, 1080p and 2160p viewports.
The timing excludes image decode, the coalescing delay and texture upload.

## Supersedes

None. Settings continues to use its existing background treatment.

## Revisit when

More than one live animated source or an additional fitted effect needs to share
the preparation budget.
