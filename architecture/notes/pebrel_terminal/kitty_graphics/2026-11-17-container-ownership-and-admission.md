# Graphics container ownership and decode admission

## Status

Implemented for review of PR #819.

## Context

The proposed PNG path accepted a container in the terminal store, upgraded its
weak reference into the frontend's owned queue, and later used the ordinary
12 MiB image decoder. A valid 4K RGBA PNG could therefore fail the 32 MiB queue
check after the terminal accepted it. Smaller containers could reach that queue
and then fail the independent 12 MiB decoder check.

The proposed cache enlargement did not fix either admission mismatch. Charging
compressed bytes alone also hid the eventual bitmap cost from the core's
resource limit.

## Evidence

The empty-queue counterexample in the #819 review is a 4096×2160 RGBA PNG larger
than 32 MiB, with a base64 representation below the 64 MiB wire limit. The new
frontend regression generates valid uncompressed PNGs at that size and at
2048×1920, and checks queue ownership, complete decoding, BGRA bytes and cache
insertion. The smaller case also exceeds the former 12 MiB decoder limit.

## Decision

The core owns retained graphics payloads. Raw pixels and PNG containers share
the existing bounded weak-reference queue and one serialized background decoder.
Waiting for a decode does not retain an additional strong container reference.
Expired resources are skipped; active decoding upgrades one reference for its
work and releases it on completion.

The core declares the 48 MiB decoded transfer-body maximum and derives the
64 MiB base64 staging bound. The PNG consumer uses that same body maximum.
Ordinary encoded images and SIXEL keep their existing 12 MiB per-item and
32 MiB owned-queue admission. The decoder retains its existing pixel and
allocation limits.

Each retained core resource is charged the greater of its encoded body size
and eventual BGRA bitmap size. The existing 16-resource/128 MiB core limits and
16-image/128 MiB frontend cache remain in force. Compressed pages cannot create
an unaccounted decoded working set merely because their PNG files are small.
These are accounting and decoder bounds, not a total process/GPU RSS guarantee.

## Rejected alternatives

- Raising the owned queue limit would retain extra source bytes while waiting
  and duplicate ownership already held by the core.
- Keeping the owned queue at 32 MiB while advertising a larger transfer cannot
  display the accepted counterexample.
- Enlarging the decoded cache cannot repair either admission check or make
  compressed-byte accounting represent bitmap cost.
- Reducing accepted PNGs to the ordinary 12 MiB image limit would discard the
  reader's valid large transfers rather than support the intended path.

## Consequences

The image representation chooses the decoder. PNG containers no longer use
ordinary encoded-image admission, while raw pixels and PNGs share lifecycle,
deduplication and slot limits. Core eviction can expire waiting work. PNG, OSC
and SIXEL retain their separate source contracts and common RGBA-to-BGRA conversion.

## Validation

Regression cases cover both size mismatches with real PNG data, bounded weak
slots, expired ownership, exact decoded pixel bytes and bitmap-cost reservations.
Existing raw-pixel, clipboard and SIXEL cases remain. Native execution results
belong to the integration's validation record.

## Supersedes

None.

## Revisit when

Graphics decoding moves to the core or the renderer introduces a unified
resource budget with explicit admission acknowledgements.
