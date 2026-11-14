# Demand-driven system video background

## Status

Private implementation under native qualification; product capability remains gated.

## Context

The Windows system decoder and mutable background texture were qualified separately.
That left no path from an MP4 file to the native background scene. COM objects also
cannot be moved between arbitrary executor threads while keeping apartment lifetime.

## Evidence

Earlier Media Foundation qualification decoded tagged 720p H.264, handled first-sample
media-type refinement, and preserved first-frame pixels across EOS/seek. Decode and
copy costs belong off UI. BGRA texture qualification retained GPU leases through
native acknowledgement; neither standalone result demonstrated video playback.

An actual 960x540 Ronova/Genshin MP4 exposed a 960x544 RGB32 converter buffer.
Rejecting every storage/display-size difference refused a valid wallpaper. The
independent first-frame oracle then confirmed a maximum RGB delta of three after
display-aperture/alignment handling; 600 decoded frames crossed two stable loops.

## Decision

Use one demand-driven owning thread per admitted video source. It creates, reads,
seeks and destroys the source reader in its own COM/MF scope. A bounded command and
reply exchange requests exactly one frame; no decoder timer or full-video collection
exists. UI presentation retains one front and at most one prefetched candidate.
Audio streams are deselected. EOS rewinds only on the next admitted request.

Reuse the common two-level byte/job authority with separate CPU budget instances.
Output leases travel with pixels and scene captures. Reserve output and conservative
native sample envelopes before reading. Native codec/reference memory is opaque;
these envelopes do not establish a bound on total decoder/process memory.

Pause revokes presentation generations and freezes the presentation clock while
preserving source progress. Source replacement/close cancels the old cursor; its
job remains charged until native worker teardown. No UI callback joins a thread.
Late results cannot replace a newer source. File and native media-type admission
precede sample decoding; changed output metadata must fit the original admission.

Keep source display size distinct from aligned storage. Accept only the declared
visible rectangle and the same or 16-pixel aligned converter dimensions. Validate
integer display apertures and every full storage row before cropping. Negative
stride reverses against storage height, including padding, rather than view height.
Never stretch padding into the background or raise the canvas/frame envelope.

## Rejected alternatives

- Start another decoder or collect every frame: duplicates policy or grows with duration.
- Move COM reader objects between executor tasks with an unsafe Send wrapper: obscures
  apartment ownership and teardown.
- Synchronously decode during paint: blocks input, resize and terminal rendering.
- Decode continuously while backgrounded: spends CPU with no presentation demand.
- Claim software H.264 qualifies every codec/platform: availability is an explicit boundary.

## Consequences

First support is silent software H.264 up to the existing 720p/60fps and 32 MiB
prototype admission. These are qualification inputs, not approved product defaults.
Native read calls are not forcibly interruptible; cancellation refuses publication
and retains admission until the actual call exits. Other platform adapters return
unsupported until implemented and executed. GL/Metal qualification is independent.

## Validation

Five actual adapter ownership/layout tests passed on Windows. The original anime
MP4 passed 600-frame decoding, two loops and a first-frame RGB delta of at most
three against the independent oracle. Four native GPUI window cases passed:
animation/loop, native minimize/restore, one-frame reduced motion, and 100 source
resets. Every case ended with known byte/job admission at zero. Physical latency
and product configuration acceptance require their own results.

## Supersedes

None. Existing draft-image and GPU-retirement decisions remain authoritative.

## Revisit when

Native read stalls require process isolation, system hardware output is qualified,
additional codecs/color formats are needed, or measured video cadence requires
direct decode into admitted GPU staging.
