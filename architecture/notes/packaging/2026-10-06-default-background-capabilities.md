# Default product background capabilities

## Status

Implemented; Windows default-feature checks, builds and native window checks
pass. This changes build inclusion, not saved activation or release publication.

## Context

The native media and WGSL paths were selected only by explicit Cargo features.
Normal product and packaging commands selected `gpui-shell` with Cargo defaults,
so successful opt-in window validation did not make those controls available in
the ordinary product build.

## Evidence

`nebula_app/Cargo.toml` defines `shader-background` through the existing GIF,
video and GPUI features. `scripts/build-windows-product.ps1` and the release
workflow use Cargo defaults. Native controllers already check saved activation
and backend support independently of feature inclusion.

## Decision

Include `shader-background` alongside `gpui-shell` in the application's default
features. Keep one feature authority in Cargo rather than append separate flags
to every installer, archive, CI and developer command. Retain the explicit
`--no-default-features --features gpui-shell` path for a media-free build.

Compilation does not authorize a source. Existing image defaults and independent
WGSL confirmation remain unchanged. Platform capability checks remain explicit;
the current Windows implementation is not advertised as a Metal or wgpu port.

## Rejected alternatives

- Update only the local validation script: ordinary products would still omit
  the validated paths.
- Duplicate feature flags throughout packaging workflows: callers could drift
  again, while the actual product default would still be incomplete.
- Enable effects in preferences with the build change: inclusion and execution
  have different ownership and user-visible cost.

## Consequences

Default builds resolve the existing optional media/compiler dependencies; no new
dependency or package asset is introduced. Platform-specific execution remains
conditional on its implemented backend. Release optimization, packaging and
other native architectures need their own validation; a local default debug
build is not a published release.

## Validation

Cargo metadata resolves the video, GIF and shader features from defaults without
additional feature arguments. The explicit media-free GPUI configuration omits
those features. The normal Windows product entry passes check and build without
extra media flags. Actual private windows display video, GIF, built-in WGSL and
terminal postprocessing; these inactive captures do not establish foreground
frame rate or optimized release performance. Architecture and revision pins
remain unchanged. No distribution package or public release was created.

## Supersedes

The opt-in build availability described in the background and terminal-effect
integration notes. Runtime activation, ABI and ownership are unchanged.

## Revisit when

A new native backend, dependency cost or distribution target needs a separately
reviewed capability boundary.
