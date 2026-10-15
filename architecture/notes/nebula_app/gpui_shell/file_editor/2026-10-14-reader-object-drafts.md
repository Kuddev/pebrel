# Reader formula and image drafts

## Status

Implemented; native visual acceptance remains pending.

## Context

The default reader must keep selectable text while formulas and images can open
an input below their rendered content. Existing live editing replaces a run and
writes each keystroke to the document. Reusing that state directly would make
Escape retain changes and would remove the formula being edited.

## Evidence

- `file_editor/block_inline.rs` identifies source-backed object runs separately
  from surrounding text; `(block, part)` distinguishes identical formulas.
- `file_editor/structure_view.rs` retains the authored block math fences outside
  the editable body range.
- `file_editor/image_cache.rs` owns decoded images, request concurrency, eviction
  and suspension; a second image loader would duplicate its ownership.
- `file_editor/outline.rs::prepare` can substitute still-image paths for rendering.
  Those paths are not authored image addresses.
- Component revision `5dd6e5b2` handles direct images but omits `ImageReference`.
  A display projection is needed before these objects have clickable bounds.

## Decision

Keep one document-owned object draft outside the canonical source buffer.
Use existing native inputs, a 100 ms background draft update, the shared literal
math parser and the existing image cache. Accept a result only when its input
entity and sequence still match. A picker uses the same identity checks.

Render the draft in the object's original position. Put its inputs in normal
layout below the containing paragraph or block, preserving inline baselines and
reserving room before the next document block. Do not add a second formula
preview or permanent helper/status copy.

Only Done or Ctrl/Cmd+Enter commits the exact original byte range through the
same document transaction used by structured editing. Check content revision,
original slice, read-only state and validation before committing. Escape/cancel
drops the draft; native IME composition settles before an Escape can close it.
Save and source-mode switching require a changed draft to be finished or canceled.
Dirty drafts participate in the existing close/reload safety checks.

Keep original image-reference targets from the parsed AST before preview rewriting.
Changing a reference occurrence writes an inline image for that occurrence only,
preserving its title and the semantic alt text. It leaves shared definitions and
other occurrences untouched. An unchanged draft preserves its original spelling.
Keep reference identity in each part and prepare each target once; expand a direct
image string only for a visible fragment. Eagerly expanding a long data URL for
every occurrence would multiply the retained document payload.

Tag completed object transactions in the existing edit history. The default reader
can undo/redo these transactions; ordinary source-mode history still requires source
mode. Keeping the tag with the changes preserves this boundary across undo and redo.

## Rejected alternatives

- Reusing keystroke-by-keystroke live editing and attempting an Escape rollback.
- Locating an object by its formula text or image URL, which aliases duplicates.
- A fixed overlay that obscures following text, or an extra formula preview pane.
- Replacing the Markdown renderer or introducing another image cache/dependency.
- Writing still-image cache paths back to the document's reference definitions.

## Consequences

Rendering and serialization remain separate; a canceled draft never enters undo
history or reaches disk. A completed edit is one document transaction. Hidden tabs
retain draft inputs while image activity and draft work follow document ownership.
The cache limit bounds retained decoded pixels, not every transient decoder allocation.

The initial UI targets existing inline object runs and structured math blocks;
ordinary text/cell editing continues to use its existing reader/live-mode boundaries.

## Validation

- `file_editor/inline_object_tests.rs` covers actual object/form bounds, repeated
  formulas, original-position drafts, cancellation, IME, invalid formulas, save,
  undo/redo, reference images, literal alt punctuation and picker cancellation.
- Architecture check and the 24 independent internationalization tests passed.
- All 99 file-editor tests passed, including real pointer movement from a formula
  to its copy control and a retained-payload regression for repeated references.
- GPUI product `cargo check --locked -p nebula --bin pebrel --features gpui-shell --tests` passed.
- No full application screenshot or manual native visual acceptance is claimed.

## Supersedes

None.

## Revisit when

The normal reader gains a shared object interface for additional container types,
or the component exposes source-aware inline rendering without splitting runs.
