# Native media qualification ownership

## Status

Implemented in the feature-gated component acceptance lab. Product GIF, video and
custom shader capabilities remain disabled pending native resource qualification.

## Context

Static image settings and packages do not establish animation playback. Replacing
images periodically also changes decoder, timer, scene and GPU resource lifetimes.
These costs require actual windows as well as deterministic scheduling tests.

## Evidence

- GPUI RenderImage is immutable; new frames receive new cache identities. Image
  cache retirement is not native GPU completion acknowledgement.
- A focus-only native probe continued preparation when minimized. Its private
  process and GPU allocations grew until the owned test process was terminated.
  Windows minimization suppresses renderer callbacks without ensuring media stop.
- Native PlatformAtlas containment defaults to false at the pinned revision.
  That observation cannot count uploads; native backend instrumentation is separate.
- image::Frames erases Send. The existing GIF decoder's concrete cursor can move
  between preparation jobs while retaining sequential disposal/composition state.
- A full-size stream fills pixel output before consuming the LZW end marker.
  Reading the next frame header directly then fails strict end-code checking.

## Decision

Use one shared source session with explicit native-window placements in a separate
acceptance executable. Static sources prepare once; only permitted dynamic sessions
own a media deadline. A Windows native gate checks visible, not minimized and
foreground on the same window. Other-platform activity fallback is labelled
incomplete and is not a product capability declaration.

Freeze the media clock during pause. A revoked generation prevents old presentation
results; it does not free the producer slot before actual completion. Source epochs
separately identify decoder progress. Same-source GIF completion may retain its
composition cursor and one candidate through pause, while a changed source discards
both. Publishing a new candidate creates a fresh current-generation deadline.

Keep at most one producer and one decoded candidate. The GIF cursor bounds input,
canvas and patch allocations before pixels, tracks restore state, and decodes in
sequence. A one-pixel sentinel drains the strict LZW end marker and rejects excess
decoded pixels. It does not collect the complete animation. Loop restarts reset
composition and detect metadata changes; this detector is not a content hash.

Geometry changes update placement. Candidate readiness alone does not notify views;
newly published pixels and playback gate changes do. Otherwise lookahead completion
causes an unnecessary unchanged-image redraw for each frame.

Keep bounded histograms and transition records. One test-control deadline and a
bounded shutdown grace period are separate from playback cadence. Prepared image
cache retirement remains explicit; no cache removal is reported as a GPU fence.

## Rejected alternatives

- Package acceptance or decoder-only tests as proof of native playback.
- Cached activation alone as a minimize/visibility signal.
- Releasing a producer slot when only its generation was cancelled.
- Discarding sequential GIF canvas progress when presentation eligibility expires.
- Whole-animation retention or decoding from a rendering callback.
- Unsupported atlas containment counters as upload/page counts.
- Increasing proposed resource limits to convert an incomplete measurement into
  acceptance.

## Consequences

The lab exercises real GIF pixels and native windows without enabling product
settings or package installation for future media. CPU source sharing and native
window texture ownership remain different costs. Known cursor capacities exclude
decoder metadata/internal allocations, scene references and total process memory.

The current image replacement path can still allocate a native atlas page per
frame. A bounded mutable texture interface requires explicit submission/retirement
ownership before it becomes a product dependency. Compiler validation and native
shader compilation are distinct from shader rendering and GPU execution budgets.

## Validation

Executed Windows cursor tests cover disposal/transparency against an independent
oracle, full-size frame-end consumption, bounded long streams, finite/infinite
loops, source changes, Send transfer and preallocation rejection. Native window
qualification covers static/ReducedMotion zero cadence, minimize/restore with frozen
clock, multi-placement/window sharing, nonclient drag, frequent resize and source
churn while a producer is outstanding. Long-running and platform-specific results
must be recorded after execution; untested platforms are not inferred from builds.

## Supersedes

None. The static wallpaper loader and package persistence boundaries remain their
existing authorities.

## Revisit when

Native GPU completion and global resource admission provide a bounded streaming
interface, all supported platforms expose qualified visibility/device events, or
real decoder/native-present evidence permits product capability activation.
