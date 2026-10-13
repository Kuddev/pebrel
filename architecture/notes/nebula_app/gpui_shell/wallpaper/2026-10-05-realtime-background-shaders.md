# Realtime background shader integration

## Status

Private integration draft. Source wiring is in progress; this revision has not
passed a product build, native interaction acceptance or performance measurement.
The public framework dependency pin and published product remain unchanged.

## Context

Video playback already has a product-owned clock and renderer ownership. The
shader qualification program renders correct native pixels but is not a product
background. Exported MP4/GIF playback does not implement realtime shader effects.

## Evidence

The preceding qualification admits a fixed-size native target, asynchronous shader
preparation, retained GPU retirement leases and a bounded fragment ABI. Its native
time oracle uses one 16-byte constant block. Those previous results do not validate
the new application controller or its new uniform/placement integration.

## Decision

- Keep WGSL as the product source language. Retired GLSL settings gain no authority.
- Add three explicit default-false built-in switches. Selection writes all effect
  flags together; storing a user path does not enable it. Custom activation requires
  a user decision. Theme installation does not authorize custom shader execution.
- Compile on the background executor with one shared outstanding compiler permit.
  A source change cancels publication, not the native/compiler work's ownership.
  Keep only the latest desired settings. Reload is explicit; no file watcher runs.
- Reuse native stream ownership and the video/shader shared 64 MiB GPU admission.
  Render into a fixed 960 by 540 target. Update a single 16-byte buffer instead of
  compiling, allocating an image or reading pixels back every frame.
- Dynamic output requests at most 20 updates per second. The native visibility,
  focus and reduced-motion gate freezes the media clock and removes the deadline.
  Grain remains static. Each window retains its own admitted native owner.
- Image, video and shader use one layout/opacity authority. Suppress video decoding
  after a shader first becomes presentable; a failed/disabled shader restores the
  underlying media instead of continuing invisible simultaneous playback.
- Preserve the original renderer constant-buffer state after the shader draw.
  Bytecode/ABI limits are structural admission, not a hard GPU execution-time bound
  for a user program. Only measured built-ins can receive performance conclusions.

## Rejected alternatives

- Present exported animations as realtime shader support.
- Recompile or allocate immutable atlas images each frame.
- Add a separate shader layout, or modify foreground terminal text/selection.
- Poll hidden windows continuously or assume a cancelled job has released resources.
- Raise existing memory budgets or re-enable obsolete language keys.

## Consequences

The optional Windows product feature still requires the private renderer overlay;
publishing needs a reviewed reproducible framework pin. Other platforms report an
unavailable capability. Source compilation, pixel correctness, switching, restart,
pause/resume, retirement, interaction latency and endurance remain distinct checks.
No performance or cross-platform acceptance is claimed for this draft.

## Validation

Pending: current-source build and focused tests, real product effect/file switching,
native pixels, paused-state work counters, memory plateau and input-latency samples.
Existing disk reserves and the user's desktop-control restriction remain in effect.

## Supersedes

Extends the static-only consumer boundary in the 2026-10-03 effect authorization
note when this integration is validated. Existing disabled/path-only semantics and
the retired-key isolation remain unchanged.

## Revisit when

The renderer API is pinned, another platform has native evidence, or measurements
justify changing the cadence/target size or the custom-program admission contract.
