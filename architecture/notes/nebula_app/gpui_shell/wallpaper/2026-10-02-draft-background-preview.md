# Bounded wallpaper preview for theme drafts

## Status

Implemented and compiled locally on Windows; native preview inspected. Other
platforms and user visual acceptance remain pending.

## Context

The theme editor initially persisted wallpaper settings but its right-hand preview
only rendered colors and text. The user requested the selected image and its
opacity/layout to appear there before applying a theme. Runtime settings must
remain unchanged while a draft is edited or cancelled.

## Evidence

The existing wallpaper loader bounds input files, decode allocations, output
pixels, and generation cancellation. GPUI `paint_image` can reuse prepared image
data with different crop and alpha values. Native preview showed that controls
alone did not demonstrate the configured background.

## Decision

Use an editor-owned GPUI entity for prepared preview state. It observes the
draft's effective wallpaper values, inheriting personal settings only when the
theme value is absent. Rendering reads that state without disk access or parsing.

Borrow the already prepared runtime image when the path matches; its original
owner retains cache cleanup. Other images use the same bounded loader and are
reduced to a maximum 512-pixel edge, at most 1 MiB of retained preview pixels.
Only one decode job per entity is outstanding; later edits replace the wanted
source and invalidate earlier generations rather than queuing every keystroke.

A background-only mutex serializes runtime and preview preparation through the
existing decode budget, including thumbnail preparation. No UI callback waits
for this mutex. It bounds simultaneous preparation, not every decoder's internal
scratch allocation or the application's total memory.

Entity release cancels pending generations and retires owned image caches.
Scalar/layout changes reuse the image without rebaking pixels. Loaded pixels
retain the BGRA channel contract. Failed previews display a localized inline
message without changing runtime preferences.

## Rejected alternatives

- Keep only a filename and an Apply instruction: does not preview the requested effect.
- Use the generic image widget's unbounded decode path: bypasses wallpaper budgets.
- Apply the draft to the real window to obtain a preview: breaks cancellation and
  the editor's Save/Apply transaction boundary.
- Retain a second full-resolution wallpaper: unnecessary for a small preview.
- Claim native window materials are reproducible inside a thumbnail: platform
  backdrops still take effect only after applying the theme.

## Consequences

Image effects are live in the editor; native window blur is applied separately.
Preview image quality is bounded, while the actual wallpaper retains its existing
runtime quality/budget. Multiple preview entities may wait for serialized work;
the UI remains asynchronous. No media decoder, shader engine, or new crate is added.

## Validation

The Windows product build passed. All 27 rendered theme-studio tests, six loader
tests, and 22 internationalization contract tests passed. Tests cover preview
painting/removal, failure feedback, the output budget and channel order, and
cancellation before opening a file. Native inspection confirmed the selected
wallpaper appears inside the actual editor preview. Screenshots remain local
acceptance artifacts; Linux/macOS and user visual acceptance are not inferred.

## Supersedes

The no-image-preview resource choice in
[background publication](../../theme_library/2026-10-02-background-theme-publication.md).
Its persistence and transaction decisions remain unchanged.

## Revisit when

Measured background-executor contention needs a different preparation queue,
portable ZIP resources need a resolver, or animation/video requires frame scheduling.
