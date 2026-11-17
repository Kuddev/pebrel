# GLSL preparation and script shader parameters

## Status

Implemented on the configuration branch. Native product compilation and GPU
presentation are separate evidence; source conversion alone does not prove either.

## Context

The native effect loaders accepted WGSL only and the programmable configuration
did not supply their settings. Renaming a file picker would leave GLSL invalid.
Animation timing was fixed, and terminal shaders had no user parameter block.

## Evidence

- Both existing loaders validate Naga IR before native preparation. Backgrounds
  admit one 16-byte uniform; terminal effects admit the frame and preceding surface.
- `BackgroundEffects::selection_updates` and `TerminalEffects::source_updates`
  already own activation and ordered-source persistence. Paths alone are not grants.
- `RawSettings` merges application-parsed defaults below explicitly saved settings.

## Decision

- Enable GLSL input and WGSL output on the existing pinned Naga dependency.
  `shader_source.rs` performs bounded source parsing without renderer ownership.
  Background IR continues through its original validator and HLSL compiler;
  terminal GLSL becomes WGSL and passes the existing GPUI resource validator.
- Select GLSL for `.glsl`/`.frag`; retain other accepted WGSL filenames. Complete
  GLSL owns its declarations; versionless `mainImage` snippets receive the product
  ABI. Includes and additional textures are not a source of file/resource access.
- Adapt `config.shaders` to the existing settings contracts, resolving paths at
  startup against the entry directory. Keep the historical `background_wgsl_*`
  persisted slots and leave retired `background_glsl_*` authorization inert.
- Shared settings own speed/frame-rate validation. Existing controllers keep
  cancellation, visibility, reduced motion, preparation limits and paint ownership.
  Speed zero requests no animation wakes; terminal output can still paint.
- Use the existing background uniform's fourth scalar. Append four terminal
  scalars after the palette, mark frame version 2 and preserve previous offsets.
  Both shader-language ABI declarations match the same CPU frame encoder.

## Rejected alternatives

- Relabeling WGSL as GLSL without a GLSL parser.
- Passing arbitrary GLSL directly to a driver and bypassing product resource limits.
- Reviving retired settings as implicit execution permission.
- Loading scripts or files from paint callbacks, or creating another playback owner.
- Adding an extensible resource graph for a small set of scalar controls.

## Consequences

Users can mix supported GLSL and WGSL terminal sources and configure both effect
types from `pebrel.luau`. Defaults remain 20 animation requests per second and speed
1; existing sources retain their field offsets. This is a bounded fragment ABI,
not complete Shadertoy compatibility or a new renderer/backend capability.

## Validation

Formatting, the architecture checker and Windows dependency resolution are checked
for this branch. Native compilation and direct configuration/shader checks are
recorded with the candidate build. Regression suites are not run at user request;
no native visual acceptance is implied by compilation.

## Supersedes

Extends the [terminal source boundary](../terminal/effects/2026-10-07-portable-source-preparation.md)
and [paint cadence](../terminal/effects/2026-10-07-backend-capability-and-paint-cadence.md)
with an input language, appended frame values and configurable timing. Their
resource ownership and backend capability decisions remain in effect.

## Revisit when

A product need requires textures, feedback buffers, additional uniform blocks or
another backend. Review that concrete resource and lifecycle contract before
expanding the accepted ABI.
