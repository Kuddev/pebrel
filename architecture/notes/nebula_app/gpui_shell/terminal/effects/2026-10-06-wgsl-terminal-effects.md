# Terminal WGSL effects and frame inputs

## Status

Windows opt-in product integration has native window evidence. Settings-control
interaction, foreground performance, extended lifecycle and other-platform
acceptance remain separate; this is not complete capability parity.

## Context

Background-only effects cannot inspect text or cursor state. The renderer now has
a scoped post-processing operation, but a window-global cursor snapshot would mix
split panes. Effects must also remain independent from the selected background.

## Evidence

TerminalElement already captures the visible grid, OSC color overrides, palette
and painted cursor. It paints completion/IME overlays separately. The native API
requires exact physical dimensions and retains resources until GPU completion.

## Decision

- Put activation, source path and animation mode in the zero-dependency settings
  authority. Source selection disables activation; neither a path nor animation
  preference authorizes execution. Background preferences are untouched.
- Give each terminal view its own controller, native owner and cursor history.
  Reuse the existing global compiler permit and GPU byte budget. Hidden terminal
  views release their GPU owner rather than accumulating textures across tabs.
- Capture data from the same snapshot used to paint. Process text, inline content
  and the cursor before completion popups, IME preedit and application overlays.
- Use a WGSL prelude with a versioned, 16-byte-block frame layout and one read-only
  surface texture. Expose time/delta/frame, focus, cursor history and theme/palette
  data. Fragment declaration order is pass order; one through eight passes share
  the renderer's two physical-size intermediate textures.
- Provide clamped pixel loads and bilinear `sample_surface` in the WGSL interface.
  The pinned Naga HLSL sampler path assumes descriptor heaps; keeping this one-layer
  surface sampling in WGSL avoids a D3D12 binding-heap or source-language adapter.
- Keep native bytecode compilation in one source implementation shared with the
  background compiler. Read files and compile only on owned background work.
- Handle cancellation, stale source/native receipts, visibility and reload without
  file reads or pipeline construction on the UI thread. Native draw submissions
  remain on the renderer's owning thread. Stop recurring animation while inactive/reduced-motion;
  the content-change mode has no recurring timer. Timestamp observations remain
  monotonic when a later content update is rendered.

## Rejected alternatives

- Reuse one global cursor for unrelated panes.
- Turn off a video/background when enabling terminal post-processing.
- Filter menus or IME by applying the effect at whole-window presentation.
- Reduce text resolution to reuse the smaller background target.
- Add another user-facing shader language or silently accept extra resource bindings.

## Consequences

The frame ABI and source limits are documented with the included effect examples.
Uniform packing is explicit little-endian data, including integer flags. Background
and terminal effects share resource pressure; an oversized surface produces a local
failure rather than an unbounded allocation. Native backend support still limits
activation; this change does not establish other-platform or full parity acceptance.

## Validation

Settings round-trip/default/activation cases pass with the shared settings suite.
The actual compiler verifies frame layout, palette indexing, sampling helpers,
ordered fragment compilation and the shipped source examples. A full Windows
product build renders the effect in real isolated windows while shell commands run.
Pixel comparisons verify terminal inversion and unchanged sidebar/titlebar regions;
path-only configuration remains disabled. A diagnostic shader observes an OSC palette
override and cursor geometry. Runtime API-created split panes retain distinct palettes
and cursor locations, and video remains present under a two-pass effect.

The clock diagnostic exposed a disagreement between GPUI activation and physical
foreground state on a non-input desktop. Uniform focus flags and animation gating
now use the same native foreground fact; the corrected native diagnostic passes.
Native owner retirement receipts are acknowledged without observed timeout/quarantine.
Frame-packing and full Scene unit regressions still need their native test build.
Real settings controls, live source switching, foreground latency and endurance
checks are not implied by these isolated-window results.

## Supersedes

None. This adds a separate post-processing channel, retaining background semantics.

## Revisit when

A further native backend, additional source composition or measured runtime costs
require an ABI, sampling or admission change.
