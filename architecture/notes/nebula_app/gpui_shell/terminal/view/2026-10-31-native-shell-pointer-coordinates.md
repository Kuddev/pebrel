# Native shell pointer coordinates

## Status

Implemented for the explicit Bash/Readline and zsh/ZLE adapter; acceptance is
limited to the verification recorded with each delivery.

## Context

Terminal copy selection has screen-cell coordinates, while native line editors
own Unicode buffer positions and undo. Replacing that buffer with a host draft
or counting simulated arrows cannot preserve their ownership.

## Evidence

The native adapter checks both the actual buffer and cursor before every edit.
Real Bash PTY output also shows that a bind-x callback clears its display line,
emits its OSC reply, then redraws the prompt and input. Reading only the grid at
reply time therefore races with the query's own redisplay.
The existing Bash/zsh startup emitted prompt-start and command lifecycle marks
but no input-start mark. The native adapter now appends a width-neutral OSC 133B
to the user's prompt after prompt-building hooks, including zsh's later precmd
hooks. Real PTY regressions cover dynamic prompt replacement and re-sourcing.

Unnumbered queries require retaining lost-reply slots to avoid late replies
answering newer gestures. A bounded queue then becomes permanently full after
enough lost replies. Native numeric arguments below Readline's verified limit
can carry and echo a query nonce without adding another transport.

## Decision

At pointer release, capture at most 4096 actual cells beginning at the active
OSC 133B boundary, plus the actual cursor. This is an explicit gesture cold path;
rendering and ordinary typing do not copy the grid. Validate the returned native
buffer against every grapheme in that clicked snapshot, including wide-cell
partners, combining marks, soft wraps and the cursor position. The edit endpoints
are native Unicode scalar indices at whole grapheme boundaries. Decline missing
or ambiguous cells, offscreen input, explicit newlines and controls.
The view uses the same already-locked `unicode-segmentation` version as terminal
input; this direct dependency avoids maintaining a second grapheme algorithm or
exposing a terminal-core API solely to forward a third-party iterator.

Keep one pending request, with a one-second validity window, unique bounded nonce,
input epoch and geometry. A new gesture replaces it; a late reply cannot consume
the replacement. Never wrap the nonce within a view lifetime. Focus loss, layout
changes, user input, settings changes and prompt/owner changes invalidate pending
gestures and validated selections.

The normal terminal selection remains the drag highlight. A validated reply moves
the native caret; Backspace/Delete delegates the selected range to the native
adapter. A fast Delete while the drag query is pending is held for that same
request, and a rejected request consumes it without deleting at another cursor.
Only this validated native edit may explicitly preserve the active prompt despite
its private key sequence; arbitrary input retains the existing control-byte gate.
Readline's redisplay also clears the terminal copy selection. While an explicitly
validated input selection exists, output wakeups may restore its highlight after
comparing its original input cells with the live grid. This comparison allocates
nothing and excludes prediction suffixes; changed input is never re-highlighted.

Capability, current native prompt and foreground/mode checks exclude AI clients,
alternate screens and application mouse reporting. Historical Agent session
identity alone does not prove an Agent still owns the pane. PowerShell capability
is not advertised by this adapter.

## Rejected alternatives

- Delay-based redraw retries cannot identify the clicked screen or native input.
- Parsing prompt text or recognizing an input box from decoration lacks ownership.
- Reusing unnumbered late-reply slots risks either retargeting or permanent lockout.
- Per-cell arrow injection and host-owned replacement strings bypass native undo.

## Consequences

Mouse editing is conservative when a shell's display cannot be verified. Existing
copy selection, application mouse reporting and native keyboard behavior retain
their paths. No file polling, persistent buffer mirror or per-frame scan is added.
The native callback remains the last check against changes occurring after the
host validated the screen snapshot.
Explicit newline/tab input, offscreen or uncertain cells, and vi keymaps retain
ordinary copy selection; they do not advertise verified editable ranges.
PowerShell/PSReadLine remains outside this adapter until its real key transport
is separately verified. AI fullscreen/application mouse behavior is unchanged.

## Validation

The shell adapter's real PTY tests cover native Unicode edits and stale-buffer/
cursor rejection. `view/shell_pointer/tests.rs` exercises production grid mapping;
`view/pointer/tests/shell_pointer.rs` drives the mounted terminal element, query
delivery, drag highlight, deletion, invalidation and application-mode exclusion.
Linux desktop verification additionally exercises both Bash and zsh with actual
mouse events: ASCII and Chinese caret insertion, range deletion and one native
undo, plus 74-column soft-wrapped input with cross-row range deletion and undo.
Test-only shell observers record the native buffer and cursor without
replacing the production adapter. This does not establish ConPTY or macOS support.

## Supersedes

None. The native protocol decision remains in
`architecture/notes/nebula_terminal/tty/2026-10-31-native-shell-pointer.md`.

## Revisit when

A line editor exposes direct screen positions, or an additional shell's native
transport and cursor semantics have been verified on its actual host.
