# Pane titles belong to pane identity

## Status

Proposed.

## Context

A split tab can contain several terminals with the same automatic title. Tab
renaming labels the whole group and cannot distinguish its individual panes.
Users need an optional pane label without changing the shell's reported title,
foreground-program identity, or the tab's existing custom name.

## Evidence

- `nebula_app/src/gpui_shell/workspace/pane_header.rs` derives each automatic
  title from the foreground program, SSH destination, or current directory.
- `TerminalPane` in `workspace.rs` owns the stable pane ID and moves with its
  terminal when tabs are rearranged or transferred between windows.
- `session_recovery.rs` maps layout leaves to individual pane metadata. The
  tab-level `custom_name` describes the group rather than any one leaf.

## Decision

Store the optional custom name on `TerminalPane`, and capture it on the matching
`LayoutSession::Pane` leaf. An absent field means the existing automatic title;
the additive, optional field does not require a session version migration.
Restore trims names through the same normalization used by the editor.

Keep title editing in `workspace/rename.rs` alongside the existing tab editor.
The pane editor resolves its target by stable pane ID, and ignores callbacks
from superseded input entities. Double-click enters editing; Enter, outside
mouse-down and blur commit; Escape discards the buffer. Outside/blur commits
preserve the user's new focus destination. On the next frame, an abandoned
input focus with no new owner falls back to the active terminal. This check and
the actual focus change happen together, without a second deferred focus call.
The outside handler also covers blank areas that do not accept focus. An empty
value means automatic naming, with the live automatic title as its placeholder;
opening and dismissing an untouched editor therefore preserves automatic mode.

Keep up to 32 previous committed names on the live pane, including the absence
of a custom name. Reopening the editor makes these available through the input's
platform Undo/Redo actions. Once typing starts, these actions pass through to
the input's native text history for the rest of that edit. This keeps a prior
rename from merging with new typing under the component's one-second undo
grouping interval. Only changed, committed names add history; Escape does not.
History stores values, never window-bound input entities, and is not persisted.

Transfers retain the pane object and its name. SSH retry replaces the terminal
object, so it explicitly carries the custom name and history forward and cancels stale
editing state. Removing the edited pane/tab discards its input. A sibling exit
preserves editing while a split header remains, and saves the surviving pane's
draft before collapsing to one pane hides that header.

## Rejected alternatives

- Reusing the tab custom name: all panes would share one label.
- Overwriting the shell title: later terminal reports could overwrite the user
  label, and consumers of program identity would receive presentation text.
- Storing names by tab index or pane ordinal: moving or closing panes changes
  these positions, which could attach a label to the wrong terminal.
- Saving the current automatic title on blur: merely opening and dismissing the
  editor would silently disable future automatic title updates.
- Seeding the input's native undo stack with prior renames: its public API cannot
  force a grouping boundary, so immediate typing could undo together with the
  previous rename. Keeping old input entities would also retain window-specific
  focus subscriptions across pane transfers.

## Consequences

Custom names override only the header text; icons and terminal identity remain
automatic. The existing split-only header visibility rule is unchanged, so a
single-pane tab retains its pane name until it is split again.

The GPUI session path preserves the new field. Older binaries and the legacy
shell can read the additive format but do not promise to retain pane names when
writing a new snapshot.

## Validation

Permanent tests in `workspace/rename/tests.rs` exercise the actual pane headers
and input controls: bounded saved-name history, repeated undo/redo, the transition
to text history, cancellation, and non-empty names through snapshots, JSON and
workspace restoration. The session schema round-trip fixture also carries named
and automatic panes. Windows preview builds were used for manual interaction
testing; the PR includes the title editor screenshot.

## Supersedes

None.

## Revisit when

Pane headers become visible for single-pane tabs, another shell adopts pane
renaming, or pane identity no longer moves with the owned terminal view.
