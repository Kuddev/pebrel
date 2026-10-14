# macOS video background as a native layer below GPUI

## Status

Implemented behind the existing `video-background` feature on macOS. Validated on
one Apple Silicon machine (macOS 27); no Intel or multi-display validation yet.

## Context

On macOS the video kind was visible in settings but fell back to image mode: the
animated backend selected the unavailable implementation, the decoder worker had no
macOS reader, and playback visibility used Win32 handles. The Windows path paints
decoded BGRA frames through `paint_stream_image`, but the pinned GPUI revision's
Metal atlas implements only postprocess resources; the stream image methods use the
trait default and fail.

## Evidence

- `gpui_macos` already places its own native blur view in the window content view,
  directly below the GPUI view. A sibling view is therefore a supported composition
  point that needs no GPUI change.
- In a standalone prototype, a 3840×2160 H.264 loop played through `AVPlayerLayer`
  with about 0.5–2% CPU in the owning process; decoding stays in the system media
  service. The integrated build was not separately profiled.
- `gpui_component::Root` fills the window with the shell color. A layer below GPUI
  was invisible at window opacity 1 until that fill was cleared; the title bar was
  the only region relying on it and showed the windows behind it once it was cleared.

## Decision

- Select an `av_player` animated backend on macOS. One `AVQueuePlayer` and
  `AVPlayerLooper` per source; each window owns a clip view inserted directly below
  the GPUI view. All AppKit and AVFoundation calls stay on the main thread.
- Painting does not touch the GPUI scene. The wallpaper paint callback only updates
  the clip frame, card radius and video frame from the shared `wallpaper_rect`, so
  fit, alignment and cover-chrome keep their image semantics.
- Preserve the image composition `w·V + a·(1-w)·B` exactly: surfaces above the video
  use alpha `a·(1-w)` and the layer uses opacity `w / (1 - a·(1-w))`
  (`underlay_composition`). While the layer is shown, the window root fill is cleared
  and the title bar paints the shell color it previously inherited.
- A 500 ms monitor owned by the source plays only when a placement window is visible
  (occlusion state, not miniaturized), the opacity is non-zero and reduced motion is
  off. It also detects decoder failure and reports the existing typed media error.

## Rejected alternatives

- Implementing Metal stream images in the GPUI fork and an AVFoundation BGRA reader:
  two repositories, a pinned-revision bump and a per-frame CPU copy and upload for
  the same visible result. It remains the route if video must enter GPUI effects.
- `paint_surface(CVPixelBuffer)`: the Metal path asserts full-range NV12, applies a
  fixed BT.601 matrix and has no opacity, corner radius or placement semantics.
- A separate process or window ordered behind Pebrel: it cannot know the card bounds,
  sidebar state or window moves without polling another process.
- Clearing `theme.background` globally: other components read it as an opaque color.

## Consequences

- The Windows admission limits (720p, 32 MiB, H.264) do not apply on macOS; the
  system decoder accepts any format AVFoundation plays, including 4K and HEVC.
- Shader backgrounds and GIF remain unavailable on macOS, as before.
- The video cannot be read back by GPUI terminal effects, which only sample the scene.

## Validation

- Unit test: `native_underlay_composites_exactly_like_a_wallpaper_painted_above_the_surface`.
- Manual (Apple Silicon, macOS 27): 4K H.264 plays muted and loops; hiding the
  application pauses and showing it resumes; the title bar and sidebar keep the shell
  color; the card shows the video with the configured fit at opacity 1 and 0.3.
- Not yet run: multiple windows, cover-chrome mode, display changes, Intel Macs.

## Supersedes

None. Adds a macOS backend at the boundary from `2026-10-06-media-backend-boundaries.md`.

## Revisit when

The pinned GPUI revision gains Metal stream images, or video must feed terminal or
background effects that sample GPUI-rendered content.
