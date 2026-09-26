# macOS command-key aliases

## Status

Implemented for issue #238.

## Context

The GPUI shell registered a macOS ⌘ shortcut set that existed nowhere else.
`bind_macos_command_keys` hand-wrote roughly two dozen bindings after
`default_workspace_bindings`, while `display::keymap::default_shortcuts()` — the
table that `clear_action` / `reset_action`, the settings keymap page and the
keybind reload path read — knew nothing about them. Half of those keys also
appear in the config macOS platform table (`config/bindings/defaults.rs`); the
other half only existed in the shell.

Two user-visible consequences followed. Clearing the Shell picker row wrote only
`Ctrl+K:ReceiveChar`, so the static ⌘K binding kept consuming the key and the
panel still opened. `Action::Quit`, declared for macOS as `"q", SUPER`, fell into
the `_ => None` arm of `workspace_binding_in_context`, so ⌘Q did nothing at all
(GPUI installs no application menu). The display layer also rendered the super
modifier as `Win+`, so the settings page and the lines the app wrote into
`pebrel_settings.txt` showed `Ctrl+Win+F` / `Win+F` on macOS.

## Evidence

- Issue #238 reproduction, with the settings file the released build wrote:
  `keybind=Ctrl+Win+F:ReceiveChar`, `keybind=Ctrl+K:ReceiveChar`,
  `keybind=shift+win+k:ReceiveChar` — the unbind persisted, yet ⌘K still opened
  the picker.
- `display::keymap` regressions (`clearing_releases_the_macos_command_alias`,
  `macos_command_aliases_round_trip_through_storage`,
  `super_modifier_renders_per_platform`) and shell regressions
  (`every_macos_command_alias_binds_a_gpui_action`,
  `released_cmd_k_is_swallowed_by_no_action`).
- GPUI keymap semantics: a no-action binding for a keystroke suppresses matching
  bindings from equal or weaker sources, and a keystroke with no remaining
  binding falls through to the view's key handler — that is why a cleared ⌘ key
  reaches the terminal instead of being eaten.
- Manual acceptance on macOS 26 (arm64, debug build, window driven through
  accessibility): default ⌘K opens the picker; clearing the row writes both
  `Ctrl+K:ReceiveChar` and `cmd+k:ReceiveChar` and releases ⌘K and Ctrl+K without
  a restart; restoring the row brings ⌘K back; ⌘Q quits with `clean_exit` true.

## Decision

`display::keymap::MACOS_COMMAND_ALIASES` is the single authority for the shell's
⌘ set. `init()` derives the static gpui bindings from it, and
`default_shortcuts()` adds it to the unbind/restore vocabulary on macOS, deduped
against entries the config table already provides for the same action and key.
The alias table writes `cmd+…` into `keybind=` lines. Existing files and shortcut
capture can still use `win+…`, and manually edited files can use `super+…` or a
different modifier order. The GPUI adapter parses and canonicalizes its binding
strings before comparing overrides and restored defaults. Display spelling must
not determine a shortcut's runtime identity.

`Action::Quit` maps to a `QuitApp` action whose handler defers
`windowing::quit_all`, the same path as tray quit, so ⌘Q saves the session and
drafts before stopping PTYs. `mods_prefix` renders the super modifier per
platform, which also corrects the spelling of lines the app writes.

## Rejected alternatives

- Showing the ⌘ aliases on the settings keymap page (reverse-looking-up the
  alias table in `effective_combo`) changes about twenty keycaps on macOS, the
  shadowing verdicts other rows derive from them, and needs its own visual
  acceptance. The page keeps showing the config table's Ctrl key.
- Moving the ⌘ set into the config macOS platform table would feed the legacy
  terminal shell UI-only actions and keep the ⌘K collision, which the table
  already resolves differently (`Esc("\x0c")` plus `ClearHistory`).
- Comparing raw display/storage spellings fails for equivalent `Win+…`,
  `Cmd+…` and `super+…` keys. Display text is not a canonical identity;
  `Ctrl++`-style display spellings also do not parse back as storage strings.
- Mapping the removed static ⌘ bindings to `ReceiveChar` from `init()` was not
  needed: the user keybind table already injects the no-action binding.

## Consequences

- Cleared actions now also write `keybind=cmd+…:ReceiveChar` lines. Users coming
  from an earlier build must clear the row once more, because their existing file
  predates the alias vocabulary and holds no line for the ⌘ half.
- The keymap page still shows Ctrl keys for actions whose macOS binding set
  includes ⌘. Releasing them works; seeing them does not.
- ⌘K after unbinding does not clear the screen. The GPUI shell has no
  clear-screen action (`Action::Esc` and `Action::ClearHistory` are unmapped) and
  the terminal encoder ignores pure ⌘ combinations, so the key is released but
  inert. Out of scope here; `Ctrl+L` still passes through to the shell.
- ⌘B and ⌘, stay static bindings: they have no editable keymap row, so there is
  nothing to clear.

## Validation

`cargo fmt --all -- --check`, `python3 scripts/check_architecture.py --base
f7ca0ec`, `python3 scripts/check_platform_cfg.py` (budget unchanged at 484) and
the `display::keymap` / `keyboard_bindings` unit suites pass; the two new
released-key tests fail without the change. Manual acceptance is listed under
Evidence. The alias table is shared data; runtime registration uses the existing
`Platform::current()` entry and remains macOS-only. The two macOS restore
regressions remain platform-gated. This avoids duplicating platform compile
branches in the UI adapter without increasing the current cfg budget.
A cross-platform GPUI dispatch regression confirms that sharing the table does
not register Command shortcuts on Windows/Linux.

The 2026-09-23 review reproduced two restore failures through GPUI keyboard
dispatch: recording Cmd+K then resetting the row, and resetting the old
`Ctrl+Win+F:ReceiveChar` override from #238. Both failed before canonicalization.
Regression coverage includes those paths and equivalent modifier spellings/order.

## Supersedes

None.

## Revisit when

The keymap page should show platform-native keycaps, or macOS ⌘K should be
assigned to a clear-screen action instead of the Shell picker.
