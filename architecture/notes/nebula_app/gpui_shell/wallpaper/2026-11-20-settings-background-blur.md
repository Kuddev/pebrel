# Shared wallpaper blur for Settings

## Status

Implemented for the GPUI Settings page.

## Context

The full-height Settings design places translucent navigation and content over
the configured wallpaper. The two surfaces continue into the title bar and
share a background image anchor.

## Evidence

The reference applies blur to both the settings sidebar and content cards, with
separate dark and light tints. Pebrel's configured wallpaper is static and the
existing loader prepares it off the UI thread. GPUI can paint the same cached
image across the entire Settings page with one sprite.

## Decision

Use the wallpaper loader's existing serialized job to prepare a second BGRA
image with a 512 px maximum edge. Derive the blur scale from the reference's
18 px setting at 1240 px. Premultiply transparent pixels during resampling and
blur so invisible RGB values cannot bleed into visible regions.

Keep the original and blurred image under the same generation, file stamp and
retirement scope. The settings title bar and body use the same themed surface
tints over the one blurred image. Fitting, alignment and opacity are applied at
paint time. Workspace extension continues to control the sharp image's scope
in the terminal workspace. A 54 px vertical gradient softens the top edge of
the Settings wallpaper using one GPUI quad.

## Rejected alternatives

- Per-surface live blur would repeatedly filter the same static wallpaper while
  Settings scrolls or resizes.
- A second full-resolution image would consume up to 8 MiB of retained pixels
  for texture detail that the filter removes.
- Native window materials sample the desktop behind the app. The wallpaper is
  an image rendered inside the app window.

## Consequences

The runtime retains up to 1 MiB of additional decoded pixels. GPUI uploads the
texture when Settings first draws it. An image with transparency can require up
to 8 MiB of transient premultiplication storage on the background executor.
Settings uses the blurred wallpaper regardless of the workspace extension
toggle. Clearing the wallpaper or setting zero opacity restores solid panels.

## Validation

Seven isolated loader tests pass, including transparent edges and output bounds.
The Linux GPUI product and test sources pass `cargo check`. The native test
build exceeded the host job's 2650 MiB memory limit before producing a runnable
binary.

On Linux ARM64 (Neoverse N1), a release-mode microbenchmark of 60 samples per
fixture measured 15.9–31.8 ms median and 16.3–32.2 ms p95 per prepared image.
Inputs were capped original textures of 1600×900, 2048×1024 and 1448×1448 px,
including translucent sources. Timing starts after decode and ends before
texture upload. Steady-state Settings painting uses a cached sprite and a
gradient quad.

## Supersedes

None.

## Revisit when

Settings needs animated imagery or measurements show first-open texture upload
or serialized preparation affecting responsiveness.
