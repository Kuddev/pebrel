# Markdown link hover and authored occurrence identity

## Status

Proposed for 2.2.1.

## Context

A rendered label, its resolved URL and the authored Markdown span represent
different data. Multiple links can share a URL; reference uses can share a
definition. Splitting paragraphs into separate link widgets would disturb
selection, wrapping and inline object layout.

## Evidence

The pinned component's inline renderer owns text hit testing, clipping and
reference resolution. The application owns document transactions and source
history. Preview image rewriting and appended reference definitions mean a
rendered byte offset is not automatically an authored document offset.

## Decision

Expose precise link hover from the component, including the resolved target
and optional source occurrence span. Window element state retains hover
across wrapped inline fragments; per-view ownership prevents an old fragment
from clearing a new fragment's hover. Reference resolution preserves the
usage span instead of substituting its definition's identity.

The editor maps identical authored link syntax by occurrence within the
owning block/part. Missing identity disables Edit while Open and Copy retain
the full resolved target. Edit captures document revision and original slice,
then uses the existing source transaction and undo history. Editing a
reference use converts that use to an inline link and leaves its neighbors
and definition unchanged. File saving stays explicit.

## Rejected alternatives

- URL-only lookup can modify the wrong duplicate occurrence.
- Paragraph splitting changes selection and line wrapping.
- Editing Cargo checkout source leaves no reproducible dependency revision.
- Reconstructing the entire Markdown document loses authored formatting.

## Consequences

The application pins the public component commit. Hover decoration changes
paint only and does not remeasure text. The action strip owns a short hide
task so the pointer can move from the link to Edit, Open or Copy; revision,
activity and mounted-view checks invalidate stale targets.

## Validation

Component tests cover wrapped hover, reference usage identity and leave after
redraw. Application fixtures exercise full target copy/open, exact occurrence
editing, cancel, unsaved changes and reader undo. Physical pointer/visual
acceptance remains distinct from headless GPUI fixtures.

## Supersedes

None.

## Revisit when

Additional preview transformations need an explicit byte mapping, or the
upstream component offers a compatible occurrence API.
