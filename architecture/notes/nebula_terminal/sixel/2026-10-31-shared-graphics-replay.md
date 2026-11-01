# Sixel through shared graphics replay

## Status

Implemented; proposed for review.

## Context

Sixel support was developed before the keyed graphics replay queue, grid transient-content
guards, parser-input wake accounting, and Kitty protocol support were merged. Replaying the old
branch's shared files would remove those changes. The integration therefore starts from
`cf6bafb21bc0d20e4ff07477522caf13add0f6e2` (the merged #767), retaining its ownership model.

## Evidence

- [StreamProcessor](../../../../nebula_terminal/src/event_loop.rs) owns pending payloads;
  [terminal replay](../../../../nebula_terminal/src/term/inline_image.rs) consumes keyed
  markers in VT order. A synchronized frame must not insert cells before it commits.
- [Real PTY tests](../../../../nebula_terminal/src/event_loop/sync_tests.rs) cover a committed
  text prefix followed by an open graphics frame. The shortest Sixel is shorter than its
  replay marker, so comparing wire bytes against parser synchronized bytes loses wakeups.
- [Decoder tests](../../../../nebula_terminal/src/sixel/tests.rs) exercise the 4096-pixel
  boundary: after 682 band advances, `@` and `N` fit while `^` and `~` do not.
- The [DEC manual, chapter 14](https://vt100.net/docs/vt3xx-gp/chapter14.html) defines six
  vertical bits per data character, opaque zero-bit backgrounds, and raster dimensions as
  minimum dimensions. Pixel aspect is separate from the stored pixel array.
- [Mixed-protocol tests](../../../../nebula_terminal/tests/inline_image/sixel.rs) verify
  OSC 1337, Kitty protocol APC and Sixel DCS in one synchronized frame at every split point.

Local scanner-only cost probe: Linux x86_64 under WSL, Rust 1.97.1 release defaults,
System allocator with allocation counting, CPU affinity 19. Each sample feeds about
32 MiB in approximately 4 KiB chunks, after one warm-up feed. Nine samples per process,
three alternating main/feature pairs produced median throughputs of 32,413/32,940 MiB/s
for plain ASCII and 691/686 MiB/s for repeated SGR/text/reset lines. Both variants made
zero steady-state allocations on these two inputs. Other build work ran concurrently;
these measurements bound the local scanner comparison, not terminal/render throughput
or a general performance guarantee. Probe sources and raw results remain local diagnostic
artifacts; correctness and bounded graphics expansion are covered by the tests above.

## Decision

Use the existing scanner to recognize 7-bit DCS Sixel and XTSMGRAPHICS. Preserve the existing
Kitty protocol APC path and the one CSI 16 t request/reply implementation. No second metrics query
handler, session message or renderer store is added for Sixel; the overlapping changes in
PR #799 are not a prerequisite.

One bounded Sixel parser performs geometry validation on the stream side and RGBA decoding
on the frontend side. The public payload carries immutable encoded bytes and validated
geometry. Limits are 12 MiB encoded, 4096 pixels per edge, 256 private color registers,
16 Mi pixels and 64 Mi pixel paint operations. Actual set bits beyond the edge are rejected;
only the unused opaque background in the final partial band is clipped at 4096.

At replay, resolve dynamic terminal background overrides, insert grid-owned image tiles,
reset text-cluster state, and mark transient grid content. The current keyed queue,
stream-owned payload lifetime and parser-input byte accounting remain authoritative.
Sixel and OSC 1337 share the existing 16-entry / 32 MiB encoded queue. The Kitty protocol retains its
separate weak raw-pixel slots and core store. GPUI serializes all decodes into the shared
16-image / 128 MiB cache, discarding expired placements before and after decoding.

Decode natural RGBA dimensions, then swap channels in place for GPUI. Pixel aspect belongs
to display geometry, so it cannot multiply the decoded allocation. The legacy shell uses
the same parser and RGBA result through its existing pane image handler; its UI-loop decode
behavior is unchanged.

Retain the previous Sixel branch's placement profile: default placement starts at the
cursor and ends on its last occupied row; a producer's following LF advances past it.
This preserves save/move/image/restore prompt layouts. Mode 8452 advances to the right.
Mode 80 follows xterm's fixed top-left behavior when set, restoring the cursor afterwards.
Below a restricted scroll region, clip rows that cannot advance rather than overwriting
the bottom source tile. Mode 1070 reports permanently private palettes.

## Rejected alternatives

- Cherry-picking obsolete shared parser/renderer files: it loses current Kitty protocol ownership,
  synchronized wake accounting and transient-grid cleanup.
- A Sixel-only cache or eager PNG conversion: it bypasses shared budgets or adds encoding
  work and allocations on the PTY path.
- Rounding every image to a whole six-row band: it rejects valid set pixels at the
  advertised 4096-pixel edge. Tight bounds for all modes also lose opaque zero-bit backgrounds.
- Stacking another CSI 16 t implementation: it risks two replies for one query.

## Consequences

This is a bounded Sixel profile, not a complete emulation of all historical DEC behavior.
Only 7-bit DCS/ST is accepted. Palettes are private; shared-palette mode is not implemented.
Each grid cell still owns one image tile: transparent Sixel preserves underlying text, but
replaces an older image tile rather than compositing multiple image resources in that cell.
The default cursor exit behavior deliberately preserves existing prompt integration rather
than claiming xterm's complete scrolling semantics.

Clearing, history eviction, alternate-screen switches, reset and resize continue to govern
image lifetimes through the shared grid. The Sixel payload does not keep grid cells alive.

## Validation

- `cargo test --locked -p nebula_terminal`: decoder, cancellation/size limits, real PTY
  wakeups, synchronized replay, private modes, palette capture, mixed-protocol budgets,
  reset/drop and grid ownership regressions.
- `cargo test --locked -p nebula --bin pebrel --features gpui-test-support inline_image`:
  natural pixels/aspect, exact BGRA/alpha/background, shared queue quotas and expired decode.
- The GPUI `protocol_image_with_real_math_bitmaps_keeps_text_visible` test exercises both
  OSC 1337 and Sixel through real stream, snapshot, decode and formula projection paths.
- `python3 scripts/check_architecture.py --base cf6bafb21bc0d20e4ff07477522caf13add0f6e2`.

The terminal suite passes 445 tests (2 existing ignored); the image filter passes 23
application tests and the formula/protocol integration passes all eight cases. Architecture,
formatting, the 55 Python governance tests, i18n contract (24 passed, 1 existing ignored),
settings (103 tests) and physical file budgets (2 tests) also pass.

The default GPUI product/test graph passes `cargo check --locked -p nebula --bin pebrel
--features gpui-shell --tests`. The legacy-only graph is already broken on unchanged base:
`legacy-shell --tests` lacks an `ashpd` async runtime; enabling `ashpd/async-io` additionally
exposes the optional `futures` import. A production diagnostic with
`--no-default-features --features legacy-shell,ashpd/async-io,futures/std` fails identically
on base and this branch because the legacy event match does not handle `KittyImage` from
#767. These unrelated legacy defects are not repaired here, and legacy acceptance is not
claimed. Native Windows/Linux GPUI image acceptance also passed, including the unchanged
OMP 18.3.3 Windows client restoring an offline image session without a forced protocol.
Native macOS visual acceptance and release CI remain separate.

## Supersedes

None. Extends the [grid-owned images decision](../term/2026-09-27-grid-owned-inline-images.md).

## Revisit when

A producer requires shared palettes, 8-bit controls, image-on-image alpha composition or
another cursor profile; or the shared replay/decode ownership and memory budgets change.
