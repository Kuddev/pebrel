# Symbol face keeps `m` for the macOS GPUI loader

## Status

Implemented for issue #870. macOS desktop visual acceptance remains separate from
the focused text-system regression.

## Context

On macOS every icon drawn with the `Pebrel Symbols` family rendered as a missing
glyph box: file-tree chevrons, folder and file icons, and other interface icons.
The regression shipped with the independent symbol face introduced in
[restore-text-metrics-and-symbol-fallback](../gpui_shell/terminal/2026-10-25-restore-text-metrics-and-symbol-fallback.md).

## Evidence

- The checked-in face covers every icon code point used by
  `display::side_panel::icons`; coverage was not the cause.
- The pinned GPUI macOS text system (`gpui_macos/src/text_system.rs`, Zed rev
  `1e716d16`) skips any added face without an `m` glyph, logging
  "has no 'm' character and was not loaded". The symbol subset deliberately had
  no Latin glyphs, so the family was never registered and `font_id` returned
  "no font found".
- A headless `MacTextSystem` test reproduced that error before the change and
  resolves the family afterwards.

## Decision

- `scripts/build_symbol_font.py` adds Maple's original `m` to the subset. Outline
  and advance are unchanged; the face grows by one glyph and 180 bytes.
- Regenerate the checked-in face from the pinned source; regeneration remains
  byte-reproducible with fontTools 4.60.2.

## Rejected alternatives

- Register complete Maple on macOS: reintroduces the 20 MB face the symbol subset
  exists to avoid.
- Patch the GPUI fork's loader: widens the fork for a property the asset can
  satisfy, and other GPUI backends could add similar checks.
- Rename the face to the loader's `Segoe Fluent Icons` exception: misleading and
  violates the independent-name requirement.
- Replace icons with SVGs: a broad UI change unrelated to the regression.

## Consequences

The symbol family now contains one Latin glyph. It stays last in configured
fallback chains, so it can supply `m` only when every earlier font lacks it.
The other Latin/CJK absence assertions remain in force.

## Validation

`font_install` tests assert the `m` glyph and advance match Maple, and on macOS
load the bundled bytes into the platform text system and resolve the file-tree
icon glyphs. Both failed before the asset change.

## Supersedes

None. The symbol-ownership decision of the linked note remains in force.

## Revisit when

GPUI's macOS loader no longer requires `m`, or interface icons move off the
symbol font.
