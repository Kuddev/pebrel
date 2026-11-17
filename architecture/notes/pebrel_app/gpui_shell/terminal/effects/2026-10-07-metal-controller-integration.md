# Metal controller integration

## Status

Native Metal source, compiler and headless execution checks passed on ARM64 and
Intel hosted macOS runners. Product dependency integration is implemented; its
current-commit locked CI is pending. Visible product acceptance remains separate.

## Context

The shared terminal controller already queries its window's postprocess capability.
The renderer's Metal implementation now supplies that capability and portable WGSL
factory, so a second application controller would duplicate ownership and settings.

## Evidence

Renderer revision `a71e1757433b6a6892042c98f89113d3c53ca799` passed
[native run 37617645140](https://github.com/Kuddev/zed/actions/runs/37617645140).
Both macOS architectures compiled complete Metal sources, compiled ordered MSL
entries and the actual 4304-byte product ABI, and executed production scene pixel
and resource-retirement checks. Both reported **Apple Paravirtual device**; this
is native Metal execution on hosted virtual graphics, not physical Mac validation.
The Windows native source/compiler/capability checks also passed in that run.

## Decision

Pin the product and all five component renderer references to that exact revision;
the component revision is `e95f47d6562b47c7b13e13a56bf4badc8a736d43`.
Add the existing Naga version to the Metal dependency's locked edges. Retain the
product's source ABI, explicit activation, eight-pass limit, background preparation,
per-pane cancellation, single-wake cadence and current-window support gate.
The renderer owns its MSL compilation/cache, two full-resolution textures and
GPU-completion-held leases. Per-invocation uniform slices use its existing frame
buffer pool, supporting repeated owner use and the full palette beyond 4 KiB.

## Rejected alternatives

A separate Mac terminal effect controller, application-native compilation, a new
source-language compatibility layer, or declaring visible acceptance from a build.

## Consequences

Default Mac product builds can reach the existing controller through the renderer
capability. This does not port background media decoders or background-only shaders.
The component's existing stale standalone lockfile remains outside this consumer
qualification; the product's complete top-level graph is checked with `--locked`.

## Validation

Native checks cover scene/overlay ordering, replay, masked input and negative
origins, repeated uniforms, eight-pass storage, actual ABI palette-tail access,
cancellation, insufficient budget, stale/foreign adoption, cache reuse and held
budget release after command completion. Current product CI and physical window,
compositor pause/resume, foreground performance and endurance remain distinct.

## Supersedes

Extends [backend capability and cadence](2026-10-07-backend-capability-and-paint-cadence.md)
at its explicit condition for qualified Metal support.

## Revisit when

Physical product evidence changes the capability or cadence assumptions, or Metal
device/surface lifetime and shared frame-buffer ownership change.
