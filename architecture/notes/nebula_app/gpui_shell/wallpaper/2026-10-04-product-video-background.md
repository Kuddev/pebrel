# Explicit video source in the real background settings

## Status

Private product integration under validation; Windows video build uses the qualified
framework overlay. Default published feature/pin remains unchanged during this work.

## Context

The native system video adapter and window renderer passed separate qualification,
but the real product could select only still images. A file path alone must not
implicitly select an expensive decoder, and repeated settings edits must not queue
one decoder per update or drop accounting before actual cancellation completes.

## Evidence

The original Ronova MP4 passed real Windows/GPUI animation, loop, native background
pause/resume, one-frame reduced motion and 100 source resets. The real program
already owns wallpaper fit, alignment, opacity and chrome scrim rules. Its global
visual state is the existing lifecycle boundary for switching and clearing a source.

## Decision

Keep the compatible background path/layout keys and add an explicit image/video kind.
Absence defaults to image. A source-kind change and path reset persist together.
Themes with an explicit old image path continue to apply image mode; unspecified
effects retain personal settings. The Windows feature uses one app-owned playback
entity, one fixed native decoder worker and one prefetched frame. Each window owns
its own stream handle and observes native activity and close.
One app-wide decoder admission is retained through actual worker teardown; a new
source waits on a background timer for that admission, with a five-second limit.
The image-only theme thumbnail skips video decoding and explains its scope; choosing
a still image in that editor explicitly sets image mode.

Reuse qualified decoding and common byte/job admission. Global budgets include
retiring sources; cancellation leaves admission with actual worker teardown. Old
video fronts remain retained until replacement has an admitted frame. Rendering
uses the existing wallpaper geometry and opacity scope. Decode, file access and
GPU completion waits remain on background workers. User-visible errors use typed
localized messages. No video source starts by default.
The feature explicitly depends on the already-locked `anyhow` and
`raw-window-handle` packages: native decode errors cross the common GPUI result
boundary, and pause admission checks the actual owning HWND.

## Rejected alternatives

- Decode any path ending in MP4: changes old configuration semantics implicitly.
- Allocate a new immutable atlas image per video frame: bypasses stream retirement.
- Store another copy of fit/opacity rules in a video renderer: creates conflicting layouts.
- Release old worker admission at cancel request: allows overlap during native calls.
- Enable unsupported platforms through a silent fallback: obscures actual capability.

## Consequences

The local Windows video feature requires the qualified framework overlay until a
reviewed pinned fork carries its stream API. Software H.264 remains the executed
decoder path; opaque codec and driver memory are outside explicit frame admission.
Product acceptance must use the full program, saved settings and visible terminal
content. Existing prototype budgets are retained as hypotheses rather than raised.

## Validation

Shared settings behavior, format roundtrip/reset, full product compilation, actual
settings controls, saved configuration, video pixels, foreground pause/resume and
window close are checked separately and recorded in the product acceptance report.

## Supersedes

None. The qualified system worker and existing wallpaper geometry remain authoritative.

## Revisit when

The fork API is pinned, native hardware decoding is qualified, other platforms gain
executed adapters, or measured full-program frame cost requires narrower invalidation.
