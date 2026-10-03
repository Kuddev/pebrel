# Light theme identities and compatibility

## Status

Implemented; native validation pending.

## Context

The default Nord family previously resolved to Paper on light systems. The light
catalog now distinguishes Nord Light, Warm Sand and Slate Light. Saved preferences
and exported custom theme documents already contain Paper and GlassLight names.

## Evidence

- `nebula_settings/src/themes.rs` owns selectable built-ins and their palettes.
- `ThemeName::from_prompt_name` reads saved names; `prompt_name` emits them.
- `display/ui/theme.rs::for_system_appearance` is shared by the shell adapters.
- Custom theme documents store concrete colors, independently of built-in defaults.
- Nord's Snow Storm and Polar Night colors are published at
  <https://github.com/nordtheme/nord/blob/develop/src/nord.scss> (MIT).

## Decision

- Keep Nord as the default dark identity and pair it with NordLight.
- Present Paper as Warm Sand and GlassLight as Slate Light. Preserve the Rust
  variants; accept both old and current saved names, emitting WarmSand/SlateLight
  on subsequent explicit selections. Loading alone does not rewrite files.
- Warm Sand remains its own light choice when following the system. Its dark
  fallback remains Nord; resolving the current appearance never overwrites the
  selected preference. Slate Light retains Glass Dark as its dark counterpart.
- Keep palette authority in settings. Nord Light uses Nord's light surfaces and
  dark text ramp, with darker accent/ANSI inks adapted for light backgrounds; it
  is an application adaptation, not an official Nord port. Warm Sand and Slate
  Light use the approved warm-neutral/amber and cool-neutral/indigo role colors.
- Preserve Paper's inherited terminal cursor and selection behavior while changing
  its default palette. Existing concrete custom-theme snapshots retain their own
  colors and explicit terminal mark fields.

## Rejected alternatives

- Dropping old names makes existing preferences silently fall back to Nord.
- Relabeling without changing palettes does not implement the new appearance.
- Rewriting all saved preferences or custom snapshots would change independent
  user choices and is unnecessary for compatibility.
- Duplicating palettes in GPUI would let terminal, chrome and previews drift.

## Consequences

Old preference files remain readable. Explicitly choosing a renamed built-in uses
its new saved name. Older application versions may not recognize those newly
written names; no bidirectional downgrade migration is claimed.

## Validation

Regression coverage checks old/new names, canonical catalog entries, light palette
readability, system appearance pairing and preserved terminal mark inheritance.
Native compilation and UI behavior checks are required before delivery.

## Supersedes

None. Historical release descriptions retain the old Nord/Paper pairing.

## Revisit when

A versioned theme identity registry replaces saved names, or a maintained official
Nord Light palette is explicitly adopted with corresponding compatibility review.
