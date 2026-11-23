# Emoji input state ownership and ordinary-text cost

## Status
Maintainer revision for PR #451; final-head CI and native glyph acceptance remain separate.

## Context
The streaming cluster fix preserves complete emoji cells, but boxing the pending
segmenter allocates once per new cluster. Taking an inline segmenter on every
continuation would instead move its entire state per codepoint. Ordinary input
must not inherit large emoji-only stack temporaries.

## Evidence
Local Windows x86_64 release probes compare main `b6e7b78d`, the original PR
integrated at `8c770233`, and the revised implementation in `term/input.rs`.
Later main application changes through `57c7bfdb` do not change this core.
The probe uses 120 x 40 cells, 1,000 history lines, four warm-up feeds, thin LTO
and one codegen unit. Timings use the default System allocator; allocation counts
come from a separate instrumented build and are not timing evidence.
Nine rounds rotate variant order on CPU affinity mask `0x1`.

Median ns per input character, including parser control bytes for VT workloads:

| Workload | Main | Original PR | Revision |
| --- | ---: | ---: | ---: |
| ASCII input | 15.625 | 14.992 | 15.088 |
| BMP CJK input | 28.323 | 29.087 | 27.935 |
| ASCII VT | 15.028 | 16.763 | 14.670 |
| ANSI VT | 12.790 | 11.764 | 11.946 |
| Reported emoji input | 57.976 | 215.737 | 198.931 |
| Long combining input | 79.558 | 79.317 | 78.869 |

The emoji workload repeats seven reported sequences 512 times per feed, for 24
measured feeds. Allocations fall from 467,625 to 381,609 (86,016 fewer, about 18.4%).
The ASCII, CJK and VT workloads allocate zero times during steady-state input.
`size_of::<Term<VoidListener>>()` is 1,848 / 1,880 / 2,008 bytes respectively:
the revision trades 128 fixed bytes per terminal against repeated heap allocation.
The ordinary-input function's `sub rsp` reservation is `0x68` / `0x168` / `0x58`;
these figures exclude pushed registers and are not total thread stack usage.

Scheduling spread is substantial. These observations support the bounded
ownership change, not a universal throughput guarantee or total RSS reduction.
Main renders these emoji incorrectly, so its emoji timing is not equal-work
correctness evidence. Probe sources and raw results remain local diagnostic data.

## Decision
Store `Option<EmojiInput>` inline and update an active segmenter in place. Only
width migration temporarily takes ownership because wrapping and cell edits
invalidate input continuity through the existing reset authority. Keep the exact
cursor and pending-wrap endpoint checks rather than introducing a second policy.

Keep the cheap negative BMP continuation check in `input_character`, and mark
`extend_emoji_input` non-inlinable so initialization and width migration temporaries
do not enlarge the ordinary-text frame. Reuse existing character placement for
ASCII, CJK, insert mode, mapped characters and wrapping.

## Rejected alternatives
- Per-cluster boxing retains avoidable allocation on emoji-heavy output.
- Taking the full inline state on every codepoint trades allocation for copying.
- An ASCII-specific placement branch duplicates established VT behavior without
  stable measured benefit; it is not retained.
- Replacing the endpoint with a continuity boolean or conditionally recording it
  weakens the existing check or adds branches without stable measured benefit.
- Allocation-instrumented timing perturbs allocation-heavy workloads and is not
  used as the final throughput comparison.

## Consequences
Each terminal has a small fixed size increase; cell size and persistence formats
are unchanged. Emoji cell tails and requested lookbehind context still allocate
when necessary. No new dependency, thread, cache or background task is introduced.
Compiler and Unicode upgrades can change both layout and the negative fast path.

## Validation
The revision passes 90 terminal-module tests, 45 reference-replay tests and 16
redraw-anchor tests, plus formatting, architecture and diff checks. Added cases
cover ASCII following an emoji, keycaps, mapped characters, insert/wrap behavior
and invalidation by cursor/grid edits. Existing chunking, copy, snapshot, selectors,
width promotion/shrink, reflow and long-mark regressions remain enabled.

An earlier complete local core run had three environment-dependent failures:
two missing-shell executable cases and one WSL-selected input fixture also
reproduced on main. They were not counted as passes or repaired by unrelated edits.
These checks do not establish real-window font coverage or end-to-end latency.

## Supersedes
The boxed-state ownership and cost discussion in
[the original cluster decision](2026-10-03-emoji-input-clusters.md).
Its segmentation, cell-width and continuity contracts remain in effect.

## Revisit when
Unicode or compiler versions change, representative production traces show an
ordinary-text regression, or terminal-count measurements make the fixed state
cost material. Preserve correctness regressions when reassessing the trade-off.
