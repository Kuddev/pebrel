# DirectWrite glyph runs keep the face that shaped them

## Status

Implemented through exact GPUI fork pins; pending review. This records why the
fix lives in the forked Windows text system rather than in Pebrel typography.

## Context

Issue #744: on Windows with the zh-CN UI, the semibold title of the close-tab
confirmation rendered as valid but unrelated CJK characters, while the regular
body and buttons on the same dialog were correct. The text and translation were
correct; only the drawn glyphs were wrong.

Pebrel registers in-memory fonts through `add_fonts` (UI font, symbol font,
bundled Maple Mono and user-imported fonts) and builds CJK fallback chains.
DirectWrite fallback mappings are created from the system font set, while
`FontId` selection searches the in-memory collection first.

## Evidence

- `gpui_windows::direct_write` attributed each glyph run to a `FontId` through
  `font_info_cache: HashMap<usize, FontId>`, keyed by the served face's COM
  address and holding no reference to that face.
- A served face is retained by `fonts` only when it is the same object as the
  face behind its `FontId`. When a family exists in both the in-memory and the
  system collection, runs are served from the system face while the `FontId`
  points at the in-memory face, so the cache keeps only an address.
- After the layout is released DirectWrite frees that face and allocates other
  faces at the same address. The stale entry rasterizes the new face's glyph
  indices with the old font: shaping is correct, attribution is not. Cold
  bold/italic faces are the ones recycled; hot regular faces stay alive, which
  matches a garbled semibold title beside a correct regular body.
- Windows CI on `windows-2022`, registering in-memory copies of installed CJK
  fonts: 4705 cache hits attributed a served face to a different family (for
  example Yu Gothic UI Bold Italic drawn as Malgun Gothic). Without in-memory
  duplicates no stale hit was observed, because every served face was the same
  object already retained by `fonts`.
- The same mechanism, with address-recycling probe output, is reported upstream
  as zed-industries/zed#64832.

## Decision

The fork's cache stores `(IDWriteFontFace3, FontId)`: a cached address keeps its
face alive, so it cannot be reused for another font while the entry exists.
The fork commit carries the regression
`glyph_runs_keep_their_served_font_after_layouts_are_released`; Pebrel and
gpui-component pin the same exact Zed revision.

## Rejected alternatives

- Removing `font_semibold` from the dialog title, or avoiding duplicate
  registrations in Pebrel: hides one trigger, while imported, bundled and
  system fonts legitimately overlap and any bold/italic run stays exposed.
- Resolving every run by family/weight/style instead of caching: adds work per
  glyph run, and the name lookup can still pick the in-memory face for glyph
  indices produced by a system face.
- Keying by an intrinsic face identity (file reference, index, simulations):
  correct but larger than the lifetime fix and harder to keep in sync upstream.
- Vendoring `gpui_windows` into Pebrel: duplicates the text-system authority
  instead of using the existing exact-revision fork workflow.

## Consequences

- The cache retains one face per distinct served object. DirectWrite's shared
  factory deduplicates live faces, so growth is bounded by the fonts actually
  drawn, like the existing `fonts` table.
- macOS and Linux text systems are unchanged.
- Pebrel source is unchanged; the change reaches users through the pins in the
  root `Cargo.toml`.

## Validation

- Before the fix the regression fails in round 0 on Windows CI; with the fix it
  passes, together with the existing DirectWrite tests.
- Interactive zh-CN acceptance on a Windows machine with the reporter's fonts
  was not performed in this change.

## Supersedes

None.

## Revisit when

Upstream Zed fixes zed-industries/zed#64832 or the maintained fork branch
absorbs this commit; then return to that fixed point and drop this pin.
