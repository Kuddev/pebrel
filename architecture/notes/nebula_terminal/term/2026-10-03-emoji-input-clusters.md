# Streaming emoji cell allocation

## Status
Proposed production fix for Issue #403; remote core evidence recorded below.

## Context
Single-codepoint input width splits ZWJ, modifier and regional-indicator emoji.
The report supplies seven exact sequences and contrasts single-codepoint emoji.

## Evidence
`Term::input` previously placed every positive-width codepoint independently.
`Cell::extra`, copying and `RenderSnapshot` already retain a head plus trailing
codepoints; GPUI sends that complete cell string to its existing text shaper.
The grid allocation prevents composition before font support can be evaluated.

## Decision
Move character placement into `term/input.rs`, a real input responsibility rather
than a file-budget split. Reuse the locked unicode-segmentation 1.13.3 UAX #29
implementation and unicode-properties 0.1.4 Emoji property (emoji feature only)
as direct core dependencies. They add no renderer dependency or new lock package.
Unicode-width 0.2.2 remains the width authority for individual bases and bounded
modifier/RI/presentation pairs. Emoji ZWJ continuations occupy two columns;
non-emoji positive-width allocation retains the existing contract.

Only a pending input endpoint is recorded for ordinary characters. A boxed
GraphemeCursor is created when an actual emoji continuation arrives; its open-ended
length consumes each UTF-8 codepoint and defers the final boundary using NextChunk. PreContext is supplied from the owning cell
only when requested; its cache then grows incrementally. Ordinary ASCII letters
and BMP CJK have no cluster allocation or property lookup. An active regression
checks every locked Emoji base against every positive-width BMP Emoji successor,
protecting the negative fast path when Unicode data changes. Combining marks do
not trigger whole-cluster width scans. Emoji presentation pairs and standard keycaps use a bounded
UTF-8 buffer; already composed emoji remain two columns.

Consecutive ownership ends at actual cursor/grid edits, alternate-buffer changes
and resize. No-op resize, SGR, synchronized output and parser chunk boundaries do
not end it. Width promotion/shrink updates the head/spacer and pending-wrap cursor
state; promotion at the last column relocates the complete cell through normal
wrapping. The leading spacer copy reads the following row and its full text.

## Rejected alternatives
A hand-maintained emoji table would drift. Per-codepoint reconstruction/width
scans make long untrusted combining output quadratic. Unconditionally merging
all graphemes into two columns changes Indic and other non-emoji width contracts.
A font substitution does not repair terminal cell allocation.

## Consequences
Existing cell extra storage carries positive-width emoji continuations too.
SGR inside one emoji retains the first cell's style and its spacer retains OSC8
links/underline color. Wrapped INSERT reserves two cells in the destination row;
non-wrapped promotion inserts only the extra cell. Wrapping at physical bottom
outside the scroll margin does not leave a nonexistent-next-row placeholder. Line-wrap-disabled edge
promotion retains clipped text without creating an out-of-bounds spacer.
This fixes model/snapshot input to the shaper; actual Windows glyph/font coverage
remains a separate native acceptance requirement.

## Validation
[Fork proof](https://github.com/WilliamWang1721/pebrel/actions/runs/37103145372)
tested source `c684654b` after the attached formatting patch: 237 core tests passed,
zero failures/ignored tests; architecture checks against `9dd3d649` passed. Tests
cover the seven original sequences and controls across UTF-8 chunks, copy/spacer
selection, snapshots, selectors/wrap/reflow, SGR/sync/OSC8, edits/DECALN/scroll,
cursor/alternate/resize resets, INSERT, bottom-margin copy and long marks.

The same fork job compared instrumented release input cost with exact main
`9dd3d649`, on Ubuntu 24.04 x86_64/Rust 1.97.1, median of five samples per unchanged workload.
The harness uses direct `Term::input`, 120×32 cells and zero scrollback, not PTY,
GPU, fonts or end-to-end UI timing. Final base/candidate ns per character:

| Workload | Base | Candidate | Base/candidate allocations |
| --- | ---: | ---: | ---: |
| ASCII (372k chars) | 8.51 | 9.00 | 0 / 0 |
| BMP CJK (200k chars) | 17.01 | 17.32 | 0 / 0 |
| Reported emoji (192k chars) | 26.94 | 102.16 | 96,000 / 228,000 |
| Long ZWJ (160,120 chars) | 33.23 | 89.73 | 160,000 / 1,000 |
| Repeated selectors (160,024 chars) | 21.53 | 57.06 | 160,112 / 242 |

Long-mark lengths 1k/2k/4k/16k measured 59.50/59.29/59.20/58.73 ns per character,
with 3,200/1,760/960/280 allocations for roughly 160k total characters. This supports
linear growth for that workload, not a universal speed guarantee. Unicode boundary
work makes emoji slower than the incorrect baseline. Lazy extra storage removes
one formerly eager Arc allocation per combining input. The earlier eager-state
candidate's 3.33× ASCII and 2.34× CJK cost was rejected and corrected.

No local builds/tests/format checks were run. Final-head upstream native/product
CI and actual Windows glyph/font acceptance are separate from this fork evidence.

## Supersedes
None. PR #431's ignored test-only reproducer is evidence, not a production fix.

## Revisit when
Unicode library versions, non-emoji grapheme allocation or font backend support
change. Reported performance applies only to the measured workload/build/runner.
