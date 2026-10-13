# Share terminal cell selection between live and mirrored terminals

## Status
Accepted for implementation.

## Context
The live terminal already had cell selection, while the desktop mirror opened a
second text dialog on long press. Duplicating selection would give the two views
different wide-character, handle, clipboard and permission behavior.

## Evidence
Both renderers use `TerminalFrame` and `TerminalCellPainter`. The desktop mirror
also has an optional reflow projection; selection must follow the displayed cells,
not the original desktop column offsets. Live zoom feedback and mirror projection
are newer main behavior that the recovered draft must retain.

## Decision
Extract the existing selection responsibility into one cell-based controller.
Keep rendering, scrolling, input ownership and projection in their existing views.
Selection holds one immutable displayed frame until dismissed, then releases the
frame and draws current output. Preserve logical endpoint identity when handles
cross, account for the initial finger-to-handle offset, and select complete wide
cells/combining sequences. Font, viewport or projection changes clear selection.
Keep soft-wrap metadata in the phone projection so copying preserves original
spaces and hard breaks without adding line breaks at the phone's column width.

Clipboard actions require an explicit user action and provide native feedback.
Paste is offered only for an authorized target; rejection keeps the selection.
The mirror can offer paste while its direct IME mode is off, through the same
existing input authority. Closing/detaching still revokes input ownership.
Keep the repository's last mirrored frame across page recreation without keeping
its reader active, and release it when the corresponding desktop is closed.

## Rejected alternatives
Copying the whole snapshot through another dialog loses cell-level interaction.
Independent controllers would duplicate Unicode and gesture rules. Replacing
current mirror code with the old draft would regress zoom and reflow behavior.

## Consequences
Selection deliberately shows a stable frame while output continues arriving.
The controller does not write terminal state, resize a remote PTY or grant input.
Retained output is bounded to the repository's existing single-frame slot.

## Validation
Existing Android view tests cover Unicode cells, changing output during selection,
handle crossing, rejected paste, readonly accessibility actions and copying a
frozen narrow projection with soft wraps, spaces and hard breaks, alongside
the existing zoom/reflow/input tests. Real touch and hardware-keyboard acceptance
remain distinct from these tests.

## Supersedes
None.

## Revisit when
The terminal frame gains semantic selection beyond its existing cell/wrap data.
