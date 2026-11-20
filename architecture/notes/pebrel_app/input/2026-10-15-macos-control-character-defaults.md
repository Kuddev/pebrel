# macOS Control characters and GUI defaults

## Status

Proposed with the fix for #656.

## Context

The GPUI workspace registered Windows-style Control shortcuts on every host and
then added macOS Command aliases. Control+K opened the Shell picker before the
terminal could receive kill-line. Control+V and character shortcuts for font size
and numbered tabs had the same ownership conflict.

## Evidence

The regression dispatches real workspace key events through TerminalView to its
owned `Msg::Input` receiver. The original implementation fails the macOS default,
settings and Control+K delivery cases. A separate regression demonstrates that
`cmd+0` and captured `cmd+digit0` can otherwise disagree about explicit ownership.

## Decision

The GPUI default source releases character keys with exactly Control on macOS.
Command aliases remain GUI defaults. Named Control+Tab keys, Control+Shift and
Control+Alt navigation, component editing bindings and explicit user bindings
retain their existing ownership. The separate OS global quick-terminal hotkey
is outside this workspace policy.

The shared keymap accepts an explicit immutable frontend policy. GPUI callers
select its Command defaults and digit-key identity; legacy callers retain their
existing model. Separate immutable caches keep this decision independent of
initialization order, compile-time feature combinations and CLI parsing.

Within GPUI, stored `digitN` and logical `N` have the same identity as the existing
runtime adapter. Default lookup, shadowing, clear and reset use that identity.
Stored rows and their ordering are unchanged; clearing one action must not
disable another action explicitly assigned to the equivalent key.

## Rejected alternatives

- Removing every Control binding would also remove established navigation and
  terminal mode controls, and require unrelated shortcut replacements.
- A global null for Control+K would interfere with explicit overrides and input
  components instead of correcting the default source.
- Changing native modifier translation or C0 encoding would alter correctly
  delivered terminal keys without fixing GUI ownership.
- Selecting a frontend from Cargo features is insufficient when a binary also
  supports `--legacy-shell`; a mutable process-wide selector is unnecessary.

## Consequences

macOS GPUI users can explicitly bind Control+K back to the picker. Removing that
override returns it to terminal input; resetting the picker restores Command+K.
Existing Control+K pass-through rows remain valid. Other hosts retain their
Control defaults. This does not add the optional ClearHistory action proposed in
#653; its macOS Ctrl+K default assertion needs to follow this platform policy.

The defaults and aliases are merged on a cold initialization path. Settings
display reuses the cached bindings; key matching introduces no new service,
dependency, persistent schema or per-event default-table rebuild.

## Validation

Focused regressions cover terminal C0 delivery, Command dispatch, explicit
overrides, removal and restoration, numeric aliases and cross-window updates.
Existing tests retain component paste, numbered-tab focus and negotiated input
coverage. Virtual GPUI events and owned shell probes do not establish physical
AppKit keyboard, arbitrary user keymaps or interactive CLI acceptance.

## Supersedes

The retained plain-Control defaults and Ctrl-only keycap display decision in
[macOS command-key aliases](../gpui_shell/2026-09-21-macos-command-key-aliases.md).
Its shared alias authority and stored-keybind contracts remain in force.

## Revisit when

The workspace changes its navigation defaults, the renderer selection changes,
or GPUI stops normalizing stored physical digit names into logical digits.
