# Client-frame reveal for day/night transitions

## Status

Implemented candidate. Windows compilation and geometry/timing tests passed in the
66aaa7e3 candidate. Native capture quality, visual behaviour and frame cost remain
pending; this integration is not yet approved for merging.

## Context

The confirmed theme transition reveals the new appearance from the window centre.
Terminal contents, input focus and layout must continue to belong to their existing
views while the animation runs. Rendering two independent workspaces would duplicate
their state and increase the cost of every transition frame.

## Evidence

The pinned GPUI exposes rectangular content masks and image cropping through
`Window::paint_image(bounds, image_bounds, ...)`. Its scene-to-image capture API is
restricted to test support. The Windows client DC offers a bounded capture
candidate without enabling test features in the product; its capture quality is
pending native validation.

See [transition ownership](../../../../../nebula_app/src/gpui_shell/theme/transition.rs),
[native capture](../../../../../nebula_app/src/platform/window_capture.rs) and
[root composition](../../../../../nebula_app/src/gpui_shell/workspace/render.rs).

## Decision

Keep transition state in each workspace. On a day/night change, retain one bounded
client snapshot and render the new theme normally. Paint non-overlapping crops of
the old image outside an expanding circle. The image is uploaded through GPUI's
existing atlas; animation frames change crop geometry without copying its pixels.

Sample the circular boundary at device-pixel centres, snap its edges and merge
equal adjacent rows before submitting crops. Use a short 220ms reveal. Capture no more than 32 MiB and
no dimension above 4096 pixels. Clear the snapshot and atlas entry after completion,
capture/drawing failure, resize, DPI change, deactivation or a close request. Repeated
switches capture the visible intermediate state before releasing the old texture.

Root composition lives in its own module, separate from workspace state and command
orchestration. It owns the overlay order above pickers, dialogs and notifications;
the transition canvas does not register an input hitbox.

## Rejected alternatives

- Re-rendering a second live workspace: duplicates view identities and ownership.
- Enabling GPUI test support for production snapshot access: couples runtime UI to
  testing infrastructure.
- A per-frame bitmap mask/upload: adds repeated pixel work and transient textures.
- A renderer fork for circular stencilling: unnecessary for this bounded candidate.

## Consequences

The initial capture is a synchronous cold-path operation, not a measured universal
performance guarantee. Temporary CPU and GPU storage includes the client bitmap,
owned snapshot and atlas texture. Boundary crop count grows with viewport height.
Frame time and capture quality require native validation at normal and high DPI.

The initial native adapter supports Windows. Other platforms, reduced motion and
capture failure preserve immediate theme switching without the reveal. A black
client capture is rejected rather than displayed as a successful snapshot.

## Validation

Geometry regressions cover the initial/full reveal, crop bounds, non-overlapping
coverage and 100–200% DPI. Four standalone Rust 1.97.1 geometry/timing tests pass
by including the production modules. This does not compile the GPUI application
or validate the Windows capture adapter. Actual compilation, theme switches, rapid reversals,
resize, input routing and GPU resource release must be checked on Windows.

## Supersedes

None.

## Revisit when

GPUI provides a production GPU snapshot and arbitrary clipping API, native capture
fails on supported presentations, or profiling shows boundary crop submission is
too costly. Replace the capture/compositing adapter while retaining view ownership.
