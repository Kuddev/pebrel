# Explicit source kind with compatible wallpaper preferences

## Status

Implemented for private product video integration; native availability remains an
application capability rather than settings-core policy.

## Context

The wallpaper path and layout preferences already persist independently of the
renderer. Video should be an explicit user choice without inferring a decoder from
a path extension or changing the meaning of an old image-only preference file.

## Evidence

Runtime settings have one shared reader, zero production dependencies and existing
reset rules. Theme effects use optional overrides: absent preserves personal values,
while an explicit empty path clears wallpaper. Native decoder tests cannot establish
schema compatibility or correct settings behavior.

## Decision

Add `background_media_kind=image|video` with Image as the default and unknown-value
fallback. Keep the existing path, opacity, fit/alignment and chrome preferences.
The UI persists a kind change and path reset as one related update. Optional theme
kind overrides roundtrip; an explicit legacy theme path applies image mode unless
the theme declares video. Clearing the path starts no decoder in either mode.

## Rejected alternatives

- Interpret MP4 suffixes automatically: changes old files and makes source roles implicit.
- Introduce another settings parser in the video UI: duplicates persistence authority.
- Add native decoder dependencies to settings: violates the shared core boundary.
- Treat absent theme kind as a video opt-in: breaks old image themes.

## Consequences

Open program windows receive the setting through the existing settings reload event.
Reset removes kind/path overrides. Saving preferences does not itself establish
native decode success; loading failures need application-owned localized feedback.

## Validation

Runtime default/parse/roundtrip and reset behavior, native theme format preservation,
settings controls and actual program reload are verified separately.

## Supersedes

None.

## Revisit when

More media kinds need explicit selection or a reviewed persistence migration replaces
the compatible wallpaper key names.
