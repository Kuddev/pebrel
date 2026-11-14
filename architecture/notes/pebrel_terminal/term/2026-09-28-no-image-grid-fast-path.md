# Keep image bookkeeping out of private text-only grids

## Status

Proposed; implemented and validated locally on Windows x64 MSVC, Rust 1.97.1.

## Context

The [grid-owned image repair](2026-09-27-grid-owned-inline-images.md) needs to
release image owners when rows leave the logical grid but remain allocated in
the reusable row cache. Its unconditional `Storage::rotate`/`shrink_lines`
cleanup also scanned text-only rows. Snapshot capture queried every visible
cell for an image, even before a terminal had ever received one.

The [PR review](https://github.com/Kuddev/pebrel/pull/316#pullrequestreview-5336698176)
requested a no-image path at the ownership boundary while retaining image
lifetime correctness and the existing cell/row layouts. Its withdrawn CJK
measurement is not evidence for this decision.

## Evidence

`Row<T>` remains a Vec and an occupied-prefix length. `Storage::swap` relies on
its four-word representation. Adding a field to every row would change that
contract. Public grid/cell mutation can introduce images without using OSC
1337, so marking only the protocol insertion path is insufficient.

Local release probes compared the PR base `ccd9b50`, reviewed head `ccb918c`,
and this refinement. All used one external source, the same package name,
byte-identical locks, opt-level 3, thin LTO and one codegen unit. A warm-up per
variant was discarded; nine measured rounds rotated/reversed variant order.
No compilation ran during measurement.

Text workloads used 120x40 cells, 10,000 history rows, 50,000 full-width CRLF
lines, 64 KiB feed chunks and 50 captures. CJK fixtures used escaped Unicode
scalars and asserted their UTF-8 bytes; ANSI used truecolor ASCII. Text/cursor
checksums matched across all three variants.

| Feed median (ms) | Base | Reviewed head | Refinement |
| --- | ---: | ---: | ---: |
| ASCII | 38.338 | 43.800 | 38.183 |
| CJK | 52.160 | 52.826 | 51.421 |
| ANSI | 45.390 | 47.485 | 44.329 |

Feed ranges (base / reviewed / refinement, ms): ASCII 36.426-41.046 /
40.920-45.510 / 37.223-39.959; CJK 51.399-53.521 / 52.280-54.743 /
50.364-52.941; ANSI 43.426-46.386 / 45.627-48.688 / 43.015-45.077.
Snapshot medians in the same order (us): ASCII 151.28/151.69/150.31,
CJK 91.26/93.17/92.03, ANSI 150.47/153.39/152.04. Ranges overlap; these are
local core-throughput observations, not app-wide or cross-platform promises.

Two image workloads compared reviewed head with the refinement: 1,000 real
OSC 1337 images of 40x4 cells, 100 history rows, with/without colored CJK and
combining text. Feed medians were 13.623/13.468 ms (images) and
13.878/13.785 ms (mixed); snapshot medians were 20.60/19.11 and 24.86/23.36 us.
Visible tile counts matched and erasure removed image fragments in both builds.

## Decision

Storage owns a conservative, monotonic transient-content guard. Public grids,
clones and deserialized grids start with cleanup enabled. Only the two empty
grids privately created by `Term::new` start without it. Protocol insertion
and `Term::grid_mut` enable it before cells can receive images. Cursor transfer
to the alternate screen enables it when the transferred template has an image.
Resize, reset and history clearing never disable it. Moving a whole grid moves
the guard with it, so no per-row state must follow reflow or the unsafe swap.

False skips the added departing-row and cell cleanup work. Snapshot capture
dispatches once to a const-specialized shared implementation; its false path
omits image lookup from the cell loop without duplicating text rules or adding
a second image traversal. The guard is private and omitted from serialization.

## Rejected alternatives

- Painter-only checks do not remove core row cleanup or snapshot work.
- Removing discard retains stale image owners in cached rows.
- An explicit image count cannot observe arbitrary assignments through the
  existing public mutable references without changing those APIs and reflow.
- A per-row image field breaks the current fixed row layout and swap contract.
- Resetting the guard on clear/reset is unsafe: a public mutable grid reference
  can reset and then insert an image without re-entering `Term::grid_mut`.
- Propagating the entire source guard on alternate-screen cursor transfer is
  unnecessary; only the template crosses that boundary, so inspect it once.

## Consequences

Cell/CellExtra/Row remain 24/40/32 bytes on the measured target. Grid grows from
192 to 200 bytes; the two guards add 16 bytes per Term, with no per-cell or
per-row allocation. Snapshot specialization trades additional generated code
for removing the per-cell branch on image-free terminals.

A grid which has held images or escaped through mutable access conservatively
retains cleanup, even after all images disappear. This is intentional under
the current mutable API; it is not a claim that exact lifetime tracking is
impossible. Live image metadata and cleanup remain necessary for correct
erasure, reflow and asynchronous decode invalidation. The eliminated work is
the unconditional charge to grids known never to have contained images.

## Validation

Passed 275 core unit tests, 12 image integration tests, 16 redraw-anchor tests,
45 reference tests and nine external ownership regressions. The latter cover
public mutation, clone, serde, cached-row eviction, history shrink, reset/reflow,
cursor-template transfer and cropped snapshot equivalence. The template-transfer
case failed on the first guard implementation and passed after its repair.
Two pre-existing shell tests were reproduced on the reviewed head (PowerShell
with the same working directory) and excluded from the focused passing run.
GPUI product/test-target compilation, no-serde core compilation, formatting,
architecture checks and 53 checker tests (three skips) also passed.

## Supersedes

None. Refines the no-image cost of the existing grid-owned lifetime decision.

## Revisit when

Image-using terminals need to regain the fast path after erasure, or raw mutable
grid access is replaced by an API capable of tracking exact image ownership.
