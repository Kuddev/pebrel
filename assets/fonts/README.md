# Bundled fonts

`MapleMonoNormal-NF-CN-Regular.ttf` remains the original complete terminal font.
Its SHA-256 is `9ae0c348a44a681fa8fa20a34e782e95316cec880102295a128b65760944c0f5`.
The GPUI shell registers it when the configured terminal, CJK, interface font
chain or theme editor draft explicitly needs it. Selecting another font does not unload an already
registered native face; a fresh process avoids that registration entirely.

`PebrelSymbols-Regular.ttf` is an independently named, OFL-licensed derivative
containing the original private-use icons and standard power symbols. It has no
Latin or CJK text glyphs. Geometry-rendered Powerline separators remain on the
terminal's existing geometry path. Other prompt and interface icons use this
small face instead of implicitly requiring the complete Maple font.

Regenerate it offline with `python scripts/build_symbol_font.py`, after installing
fontTools in the development environment. This tool is not a build or runtime
dependency. The source hash is checked before conversion; copyright/license
metadata, glyph outlines and advances are retained, and reserved font names are
replaced. See `licenses/LICENSE-PEBREL-SYMBOLS` and `THIRD-PARTY-NOTICES`.

The complete face is 20,563,672 bytes; the generated symbol face is 1,941,228 bytes
with the fontTools version used for this revision. These are font-file sizes,
not a claim about process RSS or native font-cache allocations.
