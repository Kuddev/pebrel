# Explicit streaming GIF backgrounds

## Status

Private product integration; current-source native and lifecycle acceptance pending.

## Context

The image loader intentionally exposes one still frame. The qualified GIF cursor
was used only by the media probe. Playing an exported video does not implement GIF
disposal, finite loop counts or actual animated-image selection in the product.

## Evidence

The GIF 0.14.2 reader retains ICC/XMP application extensions, including extensions
between frames. Header-only metadata checks therefore leave later allocations
outside that admission. Reopening a whole cursor also overlaps two sets of canvas,
patch and restore buffers. Existing fixture/oracle tests cover composition, loops,
full-size frames and a 10,000-frame stream; they are retained with the decoder.

## Decision

- Add explicit `gif` source selection. Existing `image` settings remain still.
- Move the cursor into the application; the qualification lab imports this exact
  implementation rather than maintaining another decoder policy.
- Validate metadata incrementally before bytes reach the codec. Preserve the
  initial header admission and bound each later extension. The structural reader
  skips encoded pixel blocks without collecting the file or animation frames.
- Reuse composition buffers across loops; recreate only the encoded reader.
- Reuse the video worker's request channel, cancellation witness, shared budgets
  and app-owned playback clock. The decoder lease is acquired before construction;
  a frame lease is acquired before decoding each output. Requested CPU admission
  remains within the existing 32 MiB per-source / 96 MiB shared budget.
- Represent finite end-of-stream separately from failures. Retain the final frame
  without a recurring timer or a restarted decoder.
- A media picker belongs to its settings entity. Changing or clearing the source
  invalidates its generation; late results do not replace newer settings.

## Rejected alternatives

- Animate every `.gif` path implicitly, changing old persisted image behavior.
- Collect all decoded frames or duplicate the video scheduling/resource state.
- Ignore GIF loop counts, silently loop forever, or treat a normal end as failure.
- Raise budgets to accommodate overlapping composition buffers.

## Consequences

The Windows optional feature uses the qualified stream renderer. Other platforms
retain explicit capability checks. Theme-editor image preview remains a still;
the main application owns animation. Byte admission does not claim a bound on total
process RSS or driver memory; those remain measured product acceptance properties.

## Validation

Pending current-source tests/build and real product checks. New regressions cover
metadata between frames and small input chunk boundaries. Existing GIF tests and
their independently generated fixture/oracle data are preserved.

## Supersedes

Promotes the GIF qualification cursor to the product's single implementation;
extends the 2026-10-04 explicit video-source and system-worker lifecycle decisions.

## Revisit when

Additional native backends are qualified or measured costs require a different
frame-size/cadence policy without weakening disposal and ownership semantics.
