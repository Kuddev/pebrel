# Linear file icons with a bounded checked-in asset set

## Status

Implemented; visual acceptance is pending. The maintainer requested linear icons,
recognizable file types and a performance-first, incremental implementation.

## Context

File lists and file tabs reused a generic file glyph. A first implementation reused
the terminal's Nerd Font mapping, but its mixed filled glyphs did not meet the
requested outline style. Phosphor's paper-plus-extension icons were then rejected:
different resource IDs did not provide the expected visual identity, especially MD.

## Evidence

Tabler Outline provides the recognizable Markdown M/arrow, Rust gear, Python,
JavaScript/TypeScript, React, Vue, Kotlin and other technology marks, alongside
consistent folder, image, archive, document and configuration symbols.
The pinned source revision, individual SVG/XML hashes and license hash are recorded
in `mobile/android/third_party/file-icons.json`.
`FileSymbols.kt` is the mobile display mapping shared by SFTP, Git and file tabs.

## Decision

Check in only the selected 50 VectorDrawable resources and preserve the original
upstream paths/viewports and rounded strokes. Refresh them explicitly with `generate_file_icons.py`;
ordinary builds do not download icons. Use static resource references and Compose's
resource caching, not runtime name lookup or a new font-based renderer.
Known technology types receive recognizable marks; image formats share a photo symbol.
Types without a selected technology mark retain their code/category symbol; unknown
extensions use the generic file. JSX/TSX use React, not a React Native label.

## Rejected alternatives

- Draw an entire icon set locally: duplicates mature geometry and visual maintenance.
- Bundle a whole icon library or add another Gradle module for this small set: no
  current requirement justifies the added build surface.
- Add SymbolCraft in this change: the existing Android resource path already handles
  the bounded selection. Its size claim is not evidence about this APK's total size.
- Mix filled brand glyphs with linear file icons: conflicts with the requested style.
- Use identical paper outlines with changing extension letters everywhere: the
  maintainer's device review rejected this as poor recognition, despite passing tests.

## Consequences

The source XML totals about 59 KiB; actual APK size and frame cost are separate
measurements. No universal speedup over XML or ImageVector is claimed. License
attribution is bundled. New file types extend the same mapping and selected assets.

## Validation

Tests distinguish MD, Rust, Python, JSON, TOML, YAML, text, PNG, ZIP and PDF resources,
check case/path handling and load real drawables. Device acceptance inspects the
actual browser and file preview header, not Android's separate system picker.

## Supersedes

None.

## Revisit when

The bounded set or supported platforms outgrow the existing resource workflow, or a
representative measured bottleneck justifies changing the loading representation.
