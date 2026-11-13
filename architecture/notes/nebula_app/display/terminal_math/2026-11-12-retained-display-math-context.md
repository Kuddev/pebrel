# Recognize clipped display math from retained terminal rows

## Status

Implemented in the shared scanner for review of [#909](https://github.com/Kuddev/pebrel/issues/909).

## Context

The terminal can still hold rows immediately above and below the painted viewport.
An already recognized display formula can be cropped, but a fresh scan used to
miss a formula if its closing delimiter was below the viewport. A separate,
deferred history scan could recover some formulas whose opener was above it.

## Evidence

The original scanner returned zero overlays with a visible `\[` and body
when `\]` was one retained row below the viewport. The first-frame regression
uses the real terminal grid and shared draw plan. It checks both edges,
including a view that holds the body with neither delimiter visible.

## Decision

[The shared scan](../../../../../nebula_app/src/display/terminal_math/scan.rs)
looks for explicit display delimiters on both sides of the viewport. It inspects
at most `min(512, ceil(16 KiB / columns) + 4)` retained rows per side and stops
at two hard blank rows. Only a side with a possible delimiter is copied into
the scan snapshot. The existing delimiter parser decides which pairs are valid.

Remember the full source span in absolute terminal rows. Rebase it to the
viewport for painting and clip there. Rows reserved by a native result and the
active input line keep their existing ownership.

## Rejected alternatives

- Require a previously painted formula: it leaves the first view of clipped
  output in source form.
- Extend the deferred unmatched-delimiter retry: it cannot start when neither
  delimiter is visible and retains a second history scan path.
- Scan the entire scrollback: the amount of work would depend on the session
  history rather than the formula source limit.

## Consequences

The scan can recover a formula before either delimiter reaches the viewport.
It still needs the terminal to hold the source rows; it cannot recover rows
that an application never sent or has overwritten. Offscreen context can
supply an explicit display block but does not become a paint region.

## Validation

On Linux ARM64, the lower-edge regression failed on the original scanner.
The changed scanner and real terminal grid passed 121 tests, with one manual
timing test ignored. This exercised the compiler and bundled math font without
building the desktop window.

On the same host, a probe optimized at level 2 scanned a 480-line synthetic log
with no formulas. These are median microseconds per scan, not full frame times:

| Viewport and position | Original | Changed |
| --- | ---: | ---: |
| 80 × 24, at bottom | 7.14 | 25.88 |
| 80 × 24, scrolling | 15.55 | 50.32 |
| 200 × 50, at bottom | 32.35 | 55.24 |
| 200 × 50, scrolling | 71.18 | 117.89 |

## Supersedes

None.

## Revisit when

Native desktop timing shows a material cost from repeated context scans, or
the terminal gains a shared row revision that can safely reuse delimiter probes.
