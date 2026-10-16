# Bindable terminal clear action (GPUI shell)

## Status

Implemented; resolves the clear-screen limitation recorded in
[2026-09-21-macos-command-key-aliases.md](2026-09-21-macos-command-key-aliases.md)
under Consequences and Revisit when.

## Context

The GPUI product shell had no clear-screen action at all. `config::Action::ClearHistory`
and the `Esc("\x0c")` sibling existed only in the legacy shell's action table
(`nebula_app/src/input/action.rs`), and the product build replaces that module with
`product_input.rs`, so the macOS config-table defaults `⌘K → Esc("\x0c")` plus
`⌘K → ClearHistory` (`nebula_app/src/config/bindings/defaults.rs`, macOS platform
section) were inert data. Runtime ⌘K belongs to the Shell picker via
`display::keymap::MACOS_COMMAND_ALIASES`. After a user cleared the Shell picker row,
`clear_action` wrote `keybind=cmd+k:ReceiveChar`, which the shell registers as
`NoAction`; ⌘K became inert. Even a hand-written `keybind=cmd+k:ClearHistory` was
dropped because `workspace_binding_in_context` had no `ClearHistory` arm, and the
settings keymap page offered no clear-screen row. Users coming from the
established macOS ⌘K clear convention had no path to that behavior.

## Evidence

- `nebula_app/src/main.rs` selects `product_input.rs` for `input` when the legacy
  shell is disabled; `input::Processor` / `process_key_bindings` are legacy-only.
- `nebula_app/src/config/bindings/defaults.rs` macOS platform table binds ⌘K twice
  (`Esc("\x0c")`, `ClearHistory`); `nebula_terminal/src/term/clear.rs` shows
  `ClearMode::Saved` clears only the scrollback while `ClearMode::All` clears the
  viewport.
- `nebula_app/src/gpui_shell/workspace/keyboard_bindings.rs`: the adapter's match
  fell through to `_ => None` for `ClearHistory` before this change; user rows are
  registered at workspace and terminal depth by `update_keybinds`.
- The 2026-09-21 alias note documents "⌘K after unbinding does not clear the
  screen … the key is released but inert" and defers the decision.

## Decision

- `nebula_workspace::ClearTerminal` is a new GPUI action. `Action::ClearHistory`
  maps to it in `workspace_binding_in_context` with the terminal key context,
  the same shape as Copy/Paste, so text inputs never lose the key.
- **No default binding is registered anywhere.** ⌘K stays on the Shell picker on
  macOS and nothing changes on Windows/Linux. The action is reachable only through
  a user `keybind=` row, now writable from the settings page: `EDITABLE_ACTIONS`
  gains a terminal-group row backed by `Action::ClearHistory` (a unit variant that
  already round-trips through `parse_action` / `action_storage_name`).
- Semantics branch on the pane's foreground state after an instant app-side wipe
  of the viewport and scrollback (`ClearMode::All` + `Saved`, zero-latency
  feedback):
  - Shell at a prompt (OSC 133 `nebula_prompt_active`): write `\x0c` to that
    pane's PTY so the shell moves the cursor home and repaints the prompt on the
    fresh screen, keeping its line state in sync.
  - Command running: the shell cannot act on `^L` (it would stay queued and
    clear the screen a second time when the command exits, wiping the output
    produced in between). Instead the view moves the terminal cursor home
    app-side (`Handler::goto(0, 0)`); sequential output continues from the top
    and the next prompt lands after the last output line.
  - Alternate screen (vim, less, other full-screen programs): the action does
    nothing — `^L` is a redraw there, not a clear.
  Clearing the grid alone leaves the cursor on its original row and the prompt
  stranded; manual acceptance rejected that. The `^L` byte is a direct PTY write
  on the focused pane, not a user-input event, so broadcast fan-out stays out of
  scope. Without shell integration the prompt probe reads false, so clears take
  the cursor-home branch; the wipe still works and the next Enter restores a
  prompt.
- The row label uses the typed message `common.clear_screen` (Simplified Chinese
  and English catalogs; other languages fall back per the catalog contract).

## Rejected alternatives

- A pure app-side grid clear with no PTY write (the first implementation):
  deterministic under any foreground program, but the cursor stays on its
  original row and the shell never repaints the prompt — after the wipe the
  screen is blank with a stray cursor, and only the next Enter restores a
  visible prompt. Manual acceptance rejected this as broken.
- An app-side cursor-home instead of `^L` at the prompt: desynchronizes the
  shell's line state from the screen and breaks cursor-addressed redraws
  (wrapped input lines, history search) without any cooperation from the shell.
- An unconditional `^L` (the exact legacy ⌘K pair): correct at the prompt, but
  while a command runs the byte stays queued in the tty input queue and the
  shell consumes it only after the command exits — clearing the screen a second
  time and wiping the output the command produced in between. The prompt-state
  probe avoids the deferred wipe.
- Scrollback-only (`ClearMode::Saved` alone, matching the legacy `ClearHistory`
  executor): leaves the screen populated; users asking for clear screen expect a
  blank viewport plus a dropped scrollback.
- Reassigning ⌘K in `MACOS_COMMAND_ALIASES` to `ClearHistory` (the common macOS
  terminal clear convention): changes shipped default behavior for every macOS
  user. This change
  deliberately preserves existing defaults; users who want it bind ⌘K themselves
  through the new row.
- Registering the config table's ⌘K `ClearHistory` default as a GPUI workspace
  default: collides with the Shell picker alias in the same keymap source tier and
  makes dispatch ambiguous.

## Consequences

- On a fresh macOS install the Clear screen row displays `Command+K`, because
  `effective_combo` reads the config table and `mods_prefix` renders `super`
  natively — while runtime ⌘K actually opens the Shell picker. This is the same
  config-table display convention recorded in the 2026-09-21 note ("the keymap
  page still shows Ctrl keys for actions whose macOS binding set includes ⌘"),
  not a new runtime claim. Capturing any key on the row writes a user row that
  wins immediately.
- Resetting the Clear screen row also removes `cmd+k:ReceiveChar` lines
  (`reset_action`'s vocabulary includes config-table defaults), which hands ⌘K
  back to the Shell picker. Clearing the row writes `keybind=cmd+k:ReceiveChar`,
  which shadows the picker's ⌘K — ordinary combo-level shadowing, identical to
  every other row.
- Windows/Linux gain an unbound row and no behavior change; existing default
  keybindings are untouched on every platform.
- Files written by earlier builds hold no `ClearHistory` line — the old shell
  could not express one — only the `ReceiveChar` leftovers of unbinding the
  Shell picker row. Those lines keep their "release the key" semantics after
  upgrading, so a pre-update clear-screen attempt stays inert until the user
  records the row once (verified against a real upgraded file: the re-record
  replaced `cmd+k:ReceiveChar` with `win+k:ReceiveChar` plus the new
  `win+k:ClearHistory`, where the later line wins). Auto-rewriting
  `ReceiveChar` lines into `ClearHistory` would overwrite an explicitly chosen
  "release the key" decision and is rejected.
- At the prompt the `^L` byte reaches whatever shell owns the normal-screen PTY,
  exactly as the legacy ⌘K pair behaved. In the running-command branch no PTY
  write happens; alternate-screen sessions are left untouched entirely.
- The legacy shell already executes `Action::ClearHistory` and is unchanged.

## Validation

`cargo fmt --all -- --check`, `cargo check -p nebula --bin pebrel --features
gpui-shell --tests --locked`, the `display::keymap` and
`gpui_shell::workspace::keyboard_bindings` test groups (including the new
adapter, dispatch-probe and label regressions), the settings-pane suite, the
i18n contract suites, `cargo test -p nebula --test file_line_budget` and
`python3 scripts/check_architecture.py --base <PR base>` pass. The dispatch
regression fails without the adapter arm; the label regression fails without the
catalog entries.

## Supersedes

None. Extends [2026-09-21-macos-command-key-aliases.md](2026-09-21-macos-command-key-aliases.md);
its alias-table authority and unbind vocabulary are reused unchanged.

## Revisit when

The keymap page renders platform-native or runtime-truth keycaps (removing the
Command+K display quirk), or the product decides clear screen deserves a default
key of its own.
