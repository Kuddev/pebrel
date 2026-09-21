# Inline source reveal with a bounded native editor

## Status

Implemented; validation results are recorded below.

## Context

A paragraph-wide source input exposes unrelated Markdown when the user clicks
one formatted word or formula. Replacing native inputs while their Markdown
changes also risks losing the IME marked range and the user's selection.
Maintaining a second unrestricted rich document would increase memory and
invalidation costs without resolving source ownership.

## Evidence

- [Projection](../../../../../nebula_app/src/gpui_shell/file_editor/inline_edit.rs)
  already preserves source ranges and untouched link destinations. A single
  revealed inline can therefore coexist with styled neighboring leaves.
- The pinned component input accepts text decorations but no embedded image or
  formula elements. Embedded objects need separate render and edit spans.
- Native input `set_value` suppresses Change events but resets local history and
  selection. It is safe for presentation changes only outside composition and
  with explicit cursor restoration. Shared document history remains authoritative.
- The pinned reader joins selected TextViews with newlines. Multiple views in
  one mixed paragraph need paragraph-aware copy assembly.

## Decision

- Keep the canonical Markdown buffer and existing document history. The active
  native input is a presentation of one source span, not another document model.
- Reveal the innermost supported inline at the collapsed caret. Translate the
  caret through source byte offsets when switching visibility; do not commit
  those switches to the source buffer or history.
- Keep the input entity stable during ordinary editing and IME composition.
  Do not reproject a noncollapsed selection or an active marked range. Native
  composition gets the first opportunity to consume Escape/Enter/history keys.
- Split paragraphs containing images or inline math at their object ranges.
  Only the active part owns a native input. Other parts retain rendered content.
  Source ranges, not formula text or image URLs, identify repeated objects.
- Retain prepared still-image paths separately from editable source ranges.
  Image preparation stays in the existing background preparation path; rendering
  does not flatten images or synchronously access the filesystem.
- Limit mixed paragraphs to 128 runs in addition to the existing 32 KiB block
  activation bound. Excessive fragmentation gets one literal source part.
- Keep selection-view references weak and evict them with the virtual rows.
  Reassemble selected runs of a top-level mixed paragraph in source order when
  copying. Retain the existing outer selection policy for structured containers.
- Compare the active input Rope before materializing a changed draft. Focus,
  selection and blink notifications do not allocate a draft String.
- Record document history by comparing the Rope with the existing history head
  and copying only the changed spans. Update that head in place. This removes
  two full-document String copies from ordinary change handling.

## Rejected alternatives

- Expanding the entire paragraph on every click fails local source reveal.
- A rendered object replaced by raw Markdown whenever a neighbor is edited
  fails the same interaction requirement for mixed paragraphs.
- Recreating native inputs or forcing projection updates during composition
  invalidates the platform's marked range.
- A WebView/DOM clone, a second full source editor, or retained per-block inputs
  introduce additional long-lived state and larger refresh surfaces.
- Per-fragment Copy with the reader's default newline joining changes text that
  was originally one paragraph.
- Trimming process working sets would change residency measurements without
  reducing the allocations retained by the document.

## Consequences

The existing document load, preview prefix, block, image and history budgets
remain separate bounds; this decision does not assert a total process RSS limit.
History comparison still scans text and insertion can move the suffix of its
String head. Structural commands, saving and undo can still materialize the
document. No general CPU percentage or memory reduction is claimed from these
code changes.

Object boundaries participate in the existing native flex/wrap layout. Table
cells retain their existing part ownership; this is not a redesign of table
navigation or the reader's global selection protocol.

## Validation

The Windows `gpui-test-support` executable passed all 86 file-editor tests.
They cover local reveal, first preview clicks, source and visible offsets,
Chinese composition, duplicate formulas, image editing, cross-fragment selection
and copying, saving and shared undo/redo. History tests include deletion of
repeated suffixes so prefix and suffix scans cannot overlap. A prepared GIF
fixture checks that mixed views retain a still preview and original source spans.

The Windows product check passed with `cargo check -p nebula --bin pebrel
--features gpui-shell --tests --locked --offline --config build.incremental=false -j 2`.

`python3 scripts/check_architecture.py --base
03ea19287156f0abcf6c9e233f72f0d0e74a5007` passed. The touched tracked files also
passed `git diff --check`. The full workspace format check and all 53 architecture
checker regression tests passed on that main-based integration tree.

## Supersedes

None.

## Revisit when

The component exposes source-aware inline layout and native embedded objects;
or profiling shows the bounded active parse or document-history scan dominates
typing latency; or table cells require independent embedded-object ownership.
