# Custom terminal key definitions

## Status
Accepted for implementation.

## Context
The mobile toolbar had a fixed key list and separate label-to-key switches.
An unknown label fell through to Down, so adding a modifier combination could
silently send a different key.

## Evidence
`LocalTerminalScreen` and `DesktopTerminalScreen` each owned their own mapping.
The native key boundary already carries a physical key, modifier mask, text and
unshifted codepoint.

## Decision
`TerminalShortcut` owns parsing/canonicalization and the shared key payload.
Persist at most 24 ordered definitions under `terminal_shortcuts/custom_v1`.
Only keys supported by the native physical-key mapping are accepted. Reject
unknown keys, duplicate modifiers and duplicate canonical combinations.
Expose the editor in terminal toolbars and input settings. Conversation controls
keep their separately supported server-side actions.

## Rejected alternatives
Raw escape strings or shell macros can execute commands and do not express
physical modifier state. Separate UI switches recreate the original divergence.

## Consequences
Custom keys apply to local, SSH and mirrored desktop terminal input. The program
and negotiated terminal keyboard protocol still determine the resulting behavior.
Removing a line removes a custom key; default controls remain available.

## Validation
`TerminalShortcutTest` covers Ctrl+Shift payloads, function/navigation keys,
modifier canonicalization and invalid input. `ShortcutEditorTest` exercises
editing, validation, order, saving, recreation and deletion.

## Supersedes
None.

## Revisit when
The native input protocol adds physical keys outside the current mapping.
