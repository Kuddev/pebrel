# Markdown ranges belong to the loaded snapshot

## Status

Implemented for document reloads.

## Context

Reopening a Markdown file starts an asynchronous read. The old outline and
live edit used to survive after the source input was cleared, so a frame
between the two load stages could slice the empty buffer with old byte ranges.

## Evidence

Ropey 2.0.0-beta.1 calls `is_char_boundary` before its bounds check in
`try_slice`. Slicing an empty rope at the old paragraph's range panics at
`rope.rs:694:22`, matching the later panics reported in #911. A range ending
inside a multibyte character returns `NonCharBoundary` in the same probe.
The first string-slicing panic in #911 has not been attributed.

## Decision

Discard the old outline and edit sessions before clearing the source. Keep the
scroll position separately and apply it when the new input and outline are
installed together. Range readers check bounds before calling Ropey.

## Rejected alternatives

Bounds checks alone would still leave old actions and ranges active while the
source is unavailable. Rechecking the file's modification time would not
prevent a frame from using those ranges during a reload.

## Consequences

The loading frame has no document rows. A successful load restores the prior
reading position within the new document; a failed read leaves no old ranges
active.

## Validation

`reloading_rewritten_markdown_replaces_the_visible_snapshot` draws before
the reload completes and after a shorter UTF-8 snapshot is installed. A
standalone probe exercises the slice guard with the locked Ropey version.

## Supersedes

None.

## Revisit when

Remove the explicit Ropey bounds workaround when the pinned version guarantees
that `try_slice` handles out-of-bounds offsets without panicking.
