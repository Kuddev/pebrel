# Native pointer ownership and reader-to-input layout

## Status

Implemented.

## Context

The reader creates one native input when a Markdown block is activated. The
first click must retain its location through that transition. Window dragging,
text dragging and the reader's edge-scroll task also share pointer events but
have different owners.

## Evidence

- A next-frame callback runs before the newly mounted input has painted. Missing
  glyph bounds previously fell back to offset zero, moving a first click to the
  beginning of the block.
- The window selection controller previously continued a drag on mouse movement
  without checking the held button. A missing or consumed release therefore
  left selection active. The reader's separate edge-scroll task needed the same
  release boundary.
- The outer reader's padding moved its recorded bottom below the visible
  viewport. The virtual list already owns the correct scroll viewport bounds.
- GPUI rendered text snaps line height to device pixels, while the native input
  used whole logical pixels. The regression reproduced a 0.5 logical pixel shift
  below the first three heading levels at 200% scale even with matching padding.
- A list's maximum height bounds its result area. Search, border and padding
  must fit in the language popup's remaining viewport space as well.

## Decision

- Keep the canonical Markdown buffer, virtual reader and single active input.
  Retain one bounded pending click, and resolve it after that input paints using
  its native UTF-8 point lookup. The input identity, content, selection, focus and
  IME range guard against an obsolete click overwriting newer input.
- Keep window selection release in the component's shared controller. Capture a
  left-button release before child handlers, and end a drag on movement without
  a held left button. Preserve the completed selection for copying.
- Give title-bar presses to window dragging. Stop the reader's owned edge-scroll
  task at the same release boundary, including when the pointer is outside the
  reader or a child consumes the event.
  Use the virtual list's viewport for edge detection instead of maintaining a
  second geometry snapshot of the padded outer container.
- Use the rendered text's device-pixel line-height rule for native input layout,
  painting and ghost text. Match heading weight and bottom spacing across reader
  and editor presentations, including headings with embedded inline objects.
- Anchor the language popup below its trigger, constrain it to the window with
  an eight-pixel margin, and reserve space for its search field and padding.
  Resize preserves keyboard search, selection and dismissal.
- Pin the component changes by exact revision in the workspace manifest and
  lockfile. A local path override is only a development aid.

## Rejected alternatives

- Guessing columns or scanning every character's bounds duplicates native hit
  testing and confuses absent geometry with a valid offset.
- Disabling Markdown selection hides the release defect and breaks normal
  dragging and copying.
- An application-only movement filter leaves the shared selection controller
  active, affecting other text surfaces and subsequent input.
- Enlarging the layout-test tolerance conceals a real reader/editor mismatch.
- Replacing the native editor, keeping inputs for every block, or adding another
  document model is unnecessary for these ownership and geometry defects.

## Consequences

The post-paint handoff is active only for the newly mounted input. It retains no
second document and no per-character geometry table. Existing preview, active
block, image and history budgets remain in force. The line-height correction
also applies to other native inputs at fractional scale factors; their tests
must remain part of validation.

## Validation

The regressions exercise actual GPUI layout and input dispatch: first-click
glyph placement with CJK and emoji, composition before initial layout, local
source reveal, normal and interrupted selection, edge scrolling, title-bar
dragging, all six heading levels at four display scales, and a language popup
that remains searchable and selectable after a window resize.

On Windows, the isolated application candidate passed all 91 file-editor tests,
the title-bar interaction regression and 67 settings tests. The component patch
passed 23 window-selection tests and 106 native-input tests. The independent
internationalization contract passed 20 tests with one existing ignored test;
its ordinary translation lookup allocation check remains green. The architecture
checker and its 53 regression tests passed. These tests do not constitute a
claim about other platforms or a total process memory limit.

## Supersedes

None. The existing bounded inline-source-reveal decision still applies.

## Revisit when

The component supplies a post-layout activation API or upstream adopts the
pointer-release and device-pixel input behavior. Remove the fork-specific patch
when an exact upstream revision passes the same interaction regressions.
