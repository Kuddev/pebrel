# Configurable tab rename uses the shared keymap

## Status

Implemented for review.

## Context

Tab rename registered F2 directly in the GPUI workspace. The editable keymap did
not contain a rename action, so users could not release that key for a terminal
application. Adding a settings row alone would leave the original F2 binding active.

## Evidence

The workspace previously registered `f2` with `RenameActiveTab`, independently
of the shared defaults and settings action list. The keymap editor previously
added a replacement chord without disabling the action's original default.

## Decision

Add the shared `RenameTab` action with default F2 and expose its localized label
in the Tabs keymap group. Both shells use the existing `keybind=combo:action`
storage format, clear/reset operations and effective-binding calculation.

Editing an action now uses one shared rebinding operation. It releases the
action's model-defined defaults and previous custom chords through `ReceiveChar`,
then assigns the replacement. Other explicit key owners are preserved. Raw
handwritten keybind entries retain their existing ordering and override semantics.

The GPUI default comes from the shared table and adapts to the existing rename
action. Runtime overrides are registered at workspace and terminal scope. Menu
shortcut hints follow the effective binding instead of a hardcoded F2 label.

## Rejected alternatives

- Add only an editable row: changes the display while F2 still intercepts input.
- Detect a particular CLI and change its keys: terminal input ownership belongs
  to the user's keymap and the application's negotiated keyboard protocol.
- Add another persistent rename-shortcut setting: duplicates the existing keymap.
- Remove the default for everyone: changes existing behavior without a user choice.

## Consequences

The default remains F2. Users can move it, clear it, or explicitly restore it;
changes apply without restarting the workspace. Keymap row edits move a binding
rather than leaving the model's previous chords as hidden aliases. Moving or
clearing the rename binding allows F2 to reach terminal applications.

## Validation

Model tests cover persistence round trips, default/custom binding release,
clear/reset and preservation of another action's explicit ownership. GPUI tests
exercise actual event dispatch in both tab layouts, menu binding lookup, the
rendered settings row and capture cancellation. A terminal-context probe verifies
F2 delivery with VT, Win32 and enhanced keyboard modes after remapping.

The explicit legacy-shell build and independent translation contracts are also
checked. Virtual-window tests do not claim an authenticated CLI session or full
desktop visual acceptance.

## Supersedes

The workspace-only F2 default and UI row edits that retained old default chords.

## Revisit when

The keymap gains explicit multiple-shortcut editing or a unified ownership model
for additional platform-specific aliases.
