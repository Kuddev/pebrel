# Bindable terminal clear action (GPUI shell)

## Status

Implemented; resolves the clear-screen limitation recorded in
[2026-09-21-macos-command-key-aliases.md](2026-09-21-macos-command-key-aliases.md)
under Consequences and Revisit when. Amended 2026-11-13: the redraw
capability contract tightened to full-sequence consumption across every
reachable keymap (review round 13, PR #653) — see the last Decision
bullet.

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
  of the viewport and scrollback (`ClearMode::All` + `Saved`):
  - Shell at a prompt (OSC 133 `nebula_prompt_active`) with the explicit
    redraw capability advertised (`pebrel_editor_redraw`, independent of the
    query readiness `pebrel_editor_ready`): write the integration-owned clear
    key — `\e[46~` bound to the readline/zle `clear-screen` function, or the
    PSReadLine `Ctrl+Shift+F9` → `ClearScreen` chord (`Ctrl+Shift+F11` stays
    with clipboard.ps1's native paste) — so the shell's own line editor
    clears the glass and repaints prompt, buffer and caret, keeping its line
    state in sync.
  - Prompt without the advertised redraw capability — no integration at all,
    a pre-redraw (2.5.3) integration whose query-only readiness says nothing
    about the unbound key, a user-owned redraw key, or a same-shell rebind
    that took the key back after the integration had owned it — or a running
    command: the view moves the terminal cursor home app-side
    (`Handler::goto(0, 0)`); sequential output continues from the top and the
    next prompt lands after the last output line. No keyboard byte is ever
    written at the prompt (see Rejected alternatives).
  - Alternate screen (vim, less, other full-screen programs): the action does
    nothing — `^L` is a redraw there, not a clear.
  Clearing the grid alone leaves the cursor on its original row and the prompt
  stranded; manual acceptance rejected that. The redraw key is a direct PTY
  write on the focused pane, not a user-input event, so broadcast fan-out stays
  out of scope. Without shell integration the prompt probe reads false, so
  clears take the cursor-home branch; the wipe still works and the next Enter
  restores a prompt.
- The row label uses the typed message `common.clear_screen` (Simplified Chinese
  and English catalogs; other languages fall back per the catalog contract).
- **Amendment (2026-11-13, review round 13):** ownership of the Bash/zsh redraw
  key is all-or-nothing across keymaps. A multi-byte chord is not consumed as a
  whole in an unbound keymap — vi(zsh) `vicmd` and readline `vi-command` leave
  the trailing `~` bound to the invert-case command, so a redraw sent into a
  merely-unbound keymap rewrote the user's line ("original" → "originaL").
  Installation therefore scans every reachable keymap and either installs the
  built-in clear-screen into each of them (vi users included) or, if any keymap
  holds a user binding, installs nothing at all; keymaps that refuse binding
  (zsh protected keymaps like `.safe`) are unreachable through a typed chord and
  are excluded from the contract rather than killing it. The per-prompt probe
  re-verifies exactly the recorded keymap set: anything that is not our
  binding — rebind, unbind, macro — revokes the announcement and the view falls
  back to the app-side cursor home.

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
- Writing `^L` at the prompt (the first implementation, rejected in review
  2026-11-12): at an OSC 133 prompt the byte is keyboard input to whatever owns
  the tty, so it invokes the user's *remapped* Ctrl+L binding, not clear screen.
  With a fixture Readline binding that inserts a marker, the marker was appended
  to live input; the dispatch test only verified the byte was sent and could not
  catch it. The owned redraw key above keeps redraw semantics independent of any
  user binding; where the channel is not advertised the action degrades to the
  app-side cursor home instead of falling back to a keyboard byte.
- Reassigning ⌘K in `MACOS_COMMAND_ALIASES` to `ClearHistory` (the common macOS
  terminal clear convention): changes shipped default behavior for every macOS
  user. This change
  deliberately preserves existing defaults; users who want it bind ⌘K themselves
  through the new row.
- Registering the config table's ⌘K `ClearHistory` default as a GPUI workspace
  default: collides with the Shell picker alias in the same keymap source tier and
  makes dispatch ambiguous.

## Consequences

- The current GPUI shortcut policy assigns Command+K to the Shell picker and
  removes competing legacy defaults. Clear screen therefore starts unbound on
  macOS as well. Clearing or resetting an unbound Clear screen row does not
  consume or release the picker's chord. An explicit user binding still wins
  through the existing keymap ownership rules.
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
- At the prompt the redraw key reaches the shell only when the integration
  explicitly owns it — query readiness (`pebrel_editor_ready`) and redraw
  ownership (`pebrel_editor_redraw`) are separate capabilities, so an upgraded
  client facing a live pre-redraw integration (query-only, key unbound) degrades
  to the app-side cursor home instead of writing the key. The redraw
  advertisement is re-verified against the live binding at every prompt and a
  same-shell rebind revokes it with an empty value, which makes the view forget
  the ownership; a binding the integration still owns keeps announcing its
  token. A user-owned redraw key — including an inert Bash string macro, which
  `bind -p`/`bind -X` do not list — withdraws only the redraw capability: query,
  completion and pointer support keep working exactly as in the pre-redraw
  releases. In the running-command branch no PTY write happens; alternate-screen
  sessions are left untouched entirely.
- The legacy shell already executes `Action::ClearHistory` and is unchanged.

## Validation

`cargo fmt --all -- --check`, `cargo check -p nebula --bin pebrel --features
gpui-shell --tests --locked`, the `display::keymap` and
`gpui_shell::workspace::keyboard_bindings` test groups (including the new
adapter, dispatch-probe and label regressions), the clear-screen view matrix
(prompt with query-only readiness — the 2.5.3 regression — with the explicit
redraw capability, with no advertisement, running command, and alternate screen;
the remapped-key case asserts no `^L` byte ever leaves the app), the
settings-pane suite, the i18n contract suites, the
`scripts/tests/test_completion_editor.py` redraw regressions (installed key
clears and keeps the buffer through a real bash/zsh PTY; a user-owned function
binding and a Bash string macro each keep the query capability alive while the
redraw advertisement stays off and the user binding keeps firing; a query-only
session leaves the key unbound; a same-shell rebind revokes the advertisement
with an empty value while a still-owned binding keeps announcing),
`cargo test -p nebula --test file_line_budget`
and `python3 scripts/check_architecture.py --base <PR base>` pass. The dispatch
regression fails without the adapter arm; the label regression fails without the
catalog entries.

## Supersedes

None. Extends [2026-09-21-macos-command-key-aliases.md](2026-09-21-macos-command-key-aliases.md);
its alias-table authority and unbind vocabulary are reused unchanged.

## Revisit when

The product decides clear screen deserves a default key of its own, or native
shell feedback shows that prompt redraw needs a different integration contract.
