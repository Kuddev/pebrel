# Rounded inline code without changing source text

## Status

Implemented. Native visual acceptance remains pending.

## Context

The reader needs rounded code surfaces and horizontal space inside inline code.
The pinned component previously painted inline backgrounds as rectangular glyph
runs. Adding spaces to Markdown or rebuilding paragraphs as independent chips
would change copied text, source offsets, wrapping or selection ownership.

## Evidence

- `file_editor/preview.rs` and `structure_inline_view.rs` use the same TextView.
- The component's `text/inline.rs` owns shaped text and selection offsets;
  `text/inline_flow.rs` measures paragraphs containing images.
- Component commit `5dd6e5b24c4384e580cacde74d42ef5db48970f8` contains the thin
  presentation patch and a source-offset/Unicode fragment regression.

## Decision

Opt the reader into `TextViewStyle::inline_code_radius`. Reserve font-relative
padding only in displayed text; map selection and link offsets back to the
unaltered source. Measure image-flow fragments with the same padded text and
paint one rounded background per contiguous visual segment. Code blocks remain
ordinary styled containers. Theme colors and reader geometry stay application-owned.

Use the existing component fork, pinned by full revision for both UI and assets.
Icon buttons have an explicit accessible name independent of their visible label.

## Rejected alternatives

- Changing Markdown or clipboard content to simulate padding.
- Replacing the paragraph selection engine or introducing a second Markdown renderer.
- Keeping a local Cargo path override or modifying Cargo's checkout cache.

## Consequences

Only opted-in visible inline text allocates a display projection. Formula parsing,
document source, persistent formats and worker ownership are unchanged. Native
input editing retains its original caret geometry; this is a reader presentation API.

## Validation

The GPUI product compiled against the local component patch. Catalog generation
validated new typed messages. Pinned-source compilation and native visual acceptance
are recorded separately in the task delivery; no relative performance claim is made.

## Supersedes

None.

## Revisit when

Upstream provides source-preserving inline padding/background decoration; replace
the thin fork adaptation after checking selection, links and image-flow wrapping.
