# Background backend ownership

## Status

Implemented; native compilation and product regression validation pending.

## Context

The integrated background entry point repeated platform selection across fields,
initialization, source replacement, rendering, and readiness. This mixed three
lifecycles: static image loading, animated actors, and native window materials.
The terminal view likewise repeated backend selection at each effect call site.

## Evidence

The integration raised the existing platform-conditional count from 468 to 520.
Forty additions were in the wallpaper entry point. The affected code owns real
decoder cancellation, last-presentable-frame retention, and GPU retirement;
removing branches without preserving those responsibilities would change behavior.

## Decision

- Keep image loading, wallpaper layout, notifications, and window enumeration in
  the shared wallpaper entry. Select the animated actor implementation once.
- Keep native animated actors in the shell because they own GPUI entities,
  subscriptions, clocks, and window-specific textures. Unsupported configurations
  retain explicit capability/error responses and create no playback resources.
- Move CPU decoding, its owning worker, and Media Foundation integration into
  `platform/background_media`. This adapter owns no application view. Existing
  decoder tests and the media lab continue to use the same production sources.
- Move DWM version detection and material application into `platform/window_material`.
  Deferred window updates and the already-applied-window set stay in the shell.
- Keep one lazy shared budget authority for media, background programs, and terminal
  effects. Preserve the original limits, preparation leases, and release ordering.
- Select the terminal effect backend at its module boundary. The unavailable
  backend has an uninhabited actor type, so it cannot publish a placeholder entity.

## Rejected alternatives

- Raising the conditional budget or changing scanner exclusions.
- Hiding platform conditions behind new configuration aliases.
- Moving GPUI view ownership into the platform directory for counting purposes.
- Enabling an unqualified backend or giving each effect a separate global budget.

## Consequences

Platform-specific fields stop leaking into shared rendering and visibility code.
The native algorithms, supported-platform matrix, and resource limits stay intact.
An unavailable terminal backend retains an empty optional handle in the view; it
does not allocate an entity, run a timer, compile a program, or paint an effect.

## Validation

The unchanged conditional checker reports 467 against the existing 468 budget.
The architecture checker passes against the integrated main baseline. Formatting
checks cover the changed Rust files. Native compilation and real window regression
results must be recorded separately; these static results do not establish them.

## Supersedes

None. Refines code ownership without changing the earlier media lifecycle decisions.

## Revisit when

A separately qualified native backend is connected to the animated or terminal
effect boundary. Preserve the common lifecycle and budget contracts at that point.
