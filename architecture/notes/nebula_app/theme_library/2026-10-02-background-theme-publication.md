# Background effects belong to the theme draft

## Status

Implemented; Windows product and rendered-control checks passed. Other-platform
and user visual acceptance remain pending.

The no-image-preview choice is superseded by
[bounded draft preview](../gpui_shell/wallpaper/2026-10-02-draft-background-preview.md).

## Context

Themes already declare wallpaper paths, image opacity, fit, alignment, and chrome
coverage. The wallpaper loader consumes runtime preferences, but applying a custom
theme previously published only window opacity, blur, cursor, typography, and
geometry. Consequently, declared wallpaper values did not reach the renderer.

## Evidence

- `theme_library/document.rs` already round-trips optional wallpaper values.
- `theme_library/preferences.rs` owns the editor/picker publication rule.
- `gpui_shell/wallpaper.rs` owns bounded background loading and window materials.
- The existing global background picker persists immediately; using it in the
  theme editor would break the editor's Save/Apply transaction boundary.

## Decision

Extend the existing preference publication rule to include all five wallpaper
values. An absent value retains the personal preference. An explicit empty image
path removes the wallpaper. Image opacity remains independent of window opacity.

Add draft-owned inputs, selects, and an asynchronous image picker to the theme
editor. Session and draft generation checks reject stale picker results. The
controls own no decoded image, texture, timer, or new resident service; applying
uses the existing loader. Reset/template replacement resynchronizes the controls.

Window blur reuses the existing native material mapping. It does not add image
blur or guarantee identical material appearance across operating systems. The UI
states that wallpaper and material changes take effect after Save and Apply.

## Rejected alternatives

- Reuse the live settings picker: writes preferences before the editor applies.
- Decode and retain another full wallpaper for these controls: adds memory and
  background-task ownership before a bounded shared preview exists.
- Interpret null as removal: breaks colors-only themes that retain personal settings.
- Add media decoders or a second GPU renderer: separate capabilities need their
  own cross-platform resource and lifecycle validation.

## Consequences

Native theme JSON can carry these preferences. Image paths still reference local
files; this change does not bundle image assets or implement ZIP installation,
GIF/video playback, shader execution, or pixel-identical cross-platform materials.

## Validation

Preference tests cover wallpaper publication, inheritance, removal, zero image
opacity, and independent window material. Rendered tests exercise the picker,
input validation, native export, Save/Apply, and a stale result after reopening.
The Windows product build and 27 rendered theme-studio tests passed, together
with 22 internationalization contract tests. Native inspection confirmed the
configured image appears in the editor. User visual acceptance and other platforms
remain separate; no screenshot acceptance is inferred from compilation.

## Supersedes

None.

## Revisit when

ZIP resources need portable references or a shared bounded preview is implemented.
