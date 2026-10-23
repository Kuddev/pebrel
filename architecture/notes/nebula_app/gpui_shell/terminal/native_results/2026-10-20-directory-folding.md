# Preserve terminal directory output when folding

## Status

Implemented locally after the maintainer canceled directory tables. Targeted
native interaction tests pass; packaged and real desktop responsiveness remain
separate acceptance steps.

## Context

Replacing ordinary directory output with widgets lost the original OSC 8 file
links, including in the widget's plain-text mode. The maintainer also reported
slow row hover and requested original terminal presentation with only the middle
collapsed: keep the first ten and last ten output rows and allow expansion.

## Evidence

Command capture already leaves characters, ANSI attributes and hyperlinks in the
VT grid. A text-only result cannot reconstruct this metadata. Existing projection
slots can shorten a source range, but hiding the entire source hides its links.
Pointer mapping also clamped the inverse-projected source row to the visual
screen size; this redirects hits on the visible tail of a compressed result.

## Decision

Recognized directory commands use an internal grid-fold presentation. Results of
at most twenty physical output rows have no slot or controls. Longer results
replace only their middle range with one control row. The retained full source
range still owns marker/fingerprint validation and eviction.

Expanded results preserve every original row and reserve one control line after
the tenth row by projecting that preceding source row into two display rows while
keeping its original cell painting enabled. Toggling preserves the viewport's
source anchor, rather than following the newly enlarged output tail.

First/tail rows continue through terminal text, color, selection, OSC 8 and pointer
rendering. Inverse pointer bounds and hover anchors use the projected source
range. Link decoration iterates that range, skips hidden rows before producing
cells and advances through sorted matches once instead of scanning all matches
for every cell. Copying a selection across the hidden middle includes its source.

## Rejected alternatives

- Show the old native Raw widget: it contains strings, not original VT attributes.
- Recreate file URIs from filenames: loses the producer's host/path semantics.
- Change the captured range to only the middle: breaks full-source validation.
- Insert or delete terminal rows: changes PTY/cursor/history semantics.

## Consequences

Fold state is per retained result and adds no persisted setting or file lookup.
Source invalidation/eviction restores original terminal output. Row counts refer
to captured physical terminal output, not inferred filesystem entries. Other
Markdown, JSON and Git presentations are unchanged.

## Validation

Real GPUI tests cover a 31-row ANSI/OSC 8 result, exactly eleven hidden middle
rows, first/tail and expanded-middle link actions, complete cross-fold drag-copy,
collapse, prompt hit mapping and absence of PTY input. A twenty-row boundary case
has no native card or fold control. Existing pointer, native-result, link underline
and command-classification checks pass. Desktop hover latency is not inferred
from synthetic-window timing.

## Supersedes

The default directory-table presentation in
`architecture/notes/nebula_terminal/inline_content/2026-10-20-directory-results-and-bounded-tables.md`.
The existing bounded capture and source-validation decisions remain.

## Revisit when

Concrete output formats or reflow behavior require a different source-line boundary,
while preserving original VT metadata and the single terminal scroll owner.
