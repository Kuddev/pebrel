# Pinned streaming renderer dependencies

## Status

Integration baseline. The published source graph resolves with Cargo's locked
Windows metadata check. Native builds and platform acceptance remain separate.

## Context

The first native background implementation used local renderer source overrides.
Saving only the application changes left an incomplete recovery point: the public
framework revision did not expose its stream owners and background shader API.

## Evidence

The required implementation spans the common scene/window/atlas contracts and
the native Direct3D and wgpu adapters. A small subset of tracked prototype files
does not capture that dependency. The component library also references the
framework directly, so changing only the application's revision creates competing
source identities.

## Decision

- Preserve the full upstream tree and ancestry; migrate renderer source changes
  to their original crate paths instead of publishing a partial standalone tree.
- Keep upstream workspace manifests and licenses. Promote wgpu shader validation
  to its normal dependency set and enable its existing IR input feature.
- Pin the renderer to `bbc49b2f512d48311917e9a6a0367be8eb00da58` and the component
  library to `858b604fd9a7dfc359b13eeb7f133079ca335480`. All component framework
  dependencies use that same renderer revision.
- Keep task-local manifest rewrites, absolute paths, screenshots and compiler
  outputs outside the dependency graph committed to the application.
- Keep WGSL as the custom effect source interface. Backend-generated HLSL/DXBC
  is an implementation detail, not an additional user-facing source language.

## Rejected alternatives

- Push an incomplete framework snapshot as though it were an upstream checkout.
- Publish absolute local dependency paths or rely on an agent's Cargo cache.
- Change only the application pin while leaving component type identities split.
- Present a successfully resolved graph as a native build or cross-platform test.

## Consequences

The baseline can be retrieved by immutable revisions without private source
overrides. Dynamic background feature flags remain explicit. The native wgpu
prototype does not establish application-level Linux or macOS activation. Future
post-processing extends WGSL capabilities without adding a GLSL source interface.

## Validation

Cargo locked offline metadata for Windows resolves the published source graph;
source migration preserves the implementation bytes apart from line endings.
Native current-source builds and lifecycle/UI acceptance are independently tracked.

## Supersedes

Replaces the local-source dependency requirement in the realtime shader and
streaming GIF integration decisions; it does not change their rendering policies.

## Revisit when

Upstream provides equivalent streaming ownership, native completion and shader
preparation, or a validated additional backend changes the required dependency set.
