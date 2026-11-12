# Bounded Git patch presentation

## Status

Implemented locally. Parser and native interaction regressions pass. Desktop
visual acceptance and hover latency measurements remain separate.

## Context

Displaying a patch as one fenced code block loses the approved file boundaries,
old/new line numbers, addition/deletion totals and file collapse. Replacing the
body with independent labels would also lose continuous text selection.

## Evidence

Git's [diff format](https://git-scm.com/docs/diff-format) distinguishes unified
hunks, extended file metadata, quoted path names and combined diffs. Combined
diffs have multiple parent columns and cannot be described by one old/new pair.
The pinned text component supports selectable literal code and non-wrapping text,
but has no per-line diff gutter API. Its button intentionally leaves focus
unchanged on mouse down; a terminal-owned focus therefore continues to send Tab
to the PTY unless the result takes focus explicitly.

## Decision

Parse retained output in the existing result-owned background task. The pure
model validates hunk counts and limits source bytes, files, lines and metadata.
It performs no Git commands or filesystem lookups. Only grid-derived output may
recover a trimmed empty context marker, and only while both hunk sides have room.
Unsupported, incomplete or oversized patches preserve their retained raw source.

Keep original source ranges, shared file bodies and row metadata. Reuse the
retained selectable code component for each body. Dual line numbers and semantic
backgrounds remain outside copied text. Match the text engine's device-snapped
line height; long lines scroll horizontally while the terminal owns vertical
scrolling. Source eviction releases the parser task, file collapse and code state.

The result prepares code entities directly; the Markdown extension wrapper enters
the same preparation method later during child layout. This avoids reborrowing
the result entity from inside its own render. Stable hover clones shared bodies
and uses cached text measurements instead of copying or parsing the patch again.

Mouse activation transfers focus to the result; keyboard activation retains the
button focus for repeated toggling. When the floating result tools cannot fit
above the viewport, leave the file collapse button's entire hit area unobstructed.

## Rejected alternatives

- Reconstruct a patch from repository state: can differ from the captured command.
- Render all patch lines as plain labels: loses continuous text selection.
- Count combined or binary-patch text as a unified hunk: produces false numbers.
- Recreate file text and entities during hover: repeats work and discards state.
- Assume a button click moves focus: routes later navigation into the terminal.

## Consequences

Each retained file owns bounded metadata and one selectable body. Raw copying
preserves headers, original signs and source endings. Rename/mode-only and binary
summary records remain visible without inventing hunk rows. Full Git binary patch
payloads and combined diffs use raw presentation.

## Validation

Three model regressions cover both line-number columns, original source ranges,
quoted UTF-8 paths, rename/mode metadata, new files, blank context rows, no-newline
markers, invalid hunks and limits. Native interaction tests exercise actual file
collapse padding, toolbar separation, Tab/Enter, repeated keyboard toggling,
complete patch copying, raw fallback and absence of PTY input. All 29 reading
tests and 15 pointer tests pass; existing Markdown/code and parser checks pass.
Translation contracts pass 24 tests with the existing manual benchmark ignored.
These checks do not substitute for actual-window visual or performance acceptance.

## Supersedes

None. Extends the existing native-result retention and block presentation rules.

## Revisit when

The component provides an equivalent selectable diff view, or combined/binary
patch presentation becomes a concrete requirement with its own source semantics.
