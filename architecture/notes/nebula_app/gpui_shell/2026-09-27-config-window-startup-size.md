# Startup window size follows `config.window.dimensions`

## Status

Implemented for issue #312 (GPUI shell, `dimensions` only).

## Context

The GPUI shell is the default product shell (`default = ["gpui-shell"]`, and
the legacy shell is a separate compile-time feature, so `config::load` is not
even in the shipped binary). Its only touch with the user's config file was
`gpui_shell::config::find_config_file()`, which resolves
`config::source::discover_toml()` and merges imports with its own TOML reader —
TOML only, never Lua, and only for the keys the settings page wants. So
`config.window.*` had no reader in the shell at all, while `pebrel config
check` — which uses `config::source::discover` + `config::load_source` —
accepted and validated keys the running app ignored. The reporter's
`config.window.dimensions = { columns = 155, lines = 43 }` on Windows 11 at
125% scaling fell into that gap: the window always opened at the shell's fixed
startup geometry.

Startup size had a second, independent cause. `workspace_window_options` pins a
1080×720 logical centered window with a 760×540 minimum, and the real grid came
from `prepare_initial_grid`, which multiplied the hard-coded
`TerminalView::DEFAULT_GRID_COLUMNS`/`DEFAULT_GRID_LINES` (116×30) by the
measured startup cell size. The 116×30 pair was itself the legacy shell's
default canvas, not a user value.

## Evidence

- `config::source::discover` / `config::load_source` are the same entry points
  `config_cli.rs` uses for `pebrel config check`, so a key accepted there must
  mean the same thing here.
- `WindowConfig::dimensions()` already carries the legacy contract "both
  nonzero, else unset"; `display::window_size` clamps each axis to
  `term::MIN_COLUMNS` / `term::MIN_SCREEN_LINES`.
- gpui rev `7c66238`: `WindowOptions::window_bounds` and `Window::resize` take
  logical `Pixels` (scaled per window by the platform layer), `App` exposes no
  scale factor, and `Window` has no `set_position`/`move_window`.
- Local `cargo check -p nebula` cannot complete on this machine: the
  `gpui_macos` build script needs the Xcode `metal` tool
  (`xcrun: error: unable to find utility "metal"`), so compile-level
  verification runs in CI. See Validation.

## Decision

`gpui_shell::config::StartupWindow` holds the startup grid and is loaded once in
`init()` through `config::source::discover(config_file)` + `config::load_source` —
the pair `pebrel config check` uses, so a key accepted there means the same thing
here, and Lua, TOML and YAML share one parser. `main` forwards the selected
`--config-file` path through `run_shell`; without that option, normal environment
and file discovery still applies. Because the value is a grid (columns × lines)
and not pixels,
`prepare_initial_grid` multiplies it by the cell it measures in the window's own
scale domain, so the result is correct at 100%, 125% and any other DPI without
the shell knowing the scale factor. Unset config keeps the 116×30 built-in
canvas; an oversized grid keeps the existing 95%-of-visible-display clamp, which
shrinks the grid rather than pushing the window off screen.

## Rejected alternatives

- Implementing `config.window.position` in the same pass. gpui only accepts
  placement through `WindowOptions::window_bounds`, in logical pixels, with no
  programmatic move and no `App`-level scale factor; the legacy shell hands winit
  a `PhysicalPosition`. Feeding the user's raw numbers as logical would place the
  window ~25% off at the reporter's 125% scaling, so `position` is left
  unwired rather than shipped wrong. `startup_mode`, `decorations` and
  `dynamic_title` are similarly untouched: they are not the reported symptom.
- Folding the field into `Settings`. `Settings` reloads on every settings-page
  write, and a Lua `require`/eval on that path would re-execute user code per
  save; the startup grid is by definition read once. Separate global, separate
  lifecycle.
- Reading `config.window.dimensions` directly in `workspace_window_options` as
  pixel math. That duplicates the chrome budget already owned by
  `prepare_initial_grid` and cannot measure the cell before a window exists, so
  the two paths would drift.
- Parsing Lua/TOML again inside `gpui_shell`. A second parser would diverge from
  `pebrel config check` in precedence and error handling; reuse keeps one
  authority.
- Extending the shell's existing TOML reader (`find_config_file` +
  `load_merged_toml`) with a `window.dimensions` lookup. It cannot see a Lua
  file, which is the format the reported config uses, and it deserializes into
  `toml::Value` rather than `UiConfig`, so one key would need a third spelling
  of the same config.
- Fixing only the legacy shell, or documenting the key as legacy-only. The
  default product shell is the one users hit.

## Consequences

- The GPUI shell now depends on the app-level config loader (`config::source`,
  `config::load_source`, and therefore mlua for Lua sources) during bootstrap.
  Discovery order stays `pebrel.lua` → `pebrel.toml` → YAML, so a Lua file
  shadows a TOML file for startup size exactly as it does for `config check`.
- `init()` performs one synchronous config read plus possible Lua evaluation on
  the main thread before the first window opens. The explicit CLI path reaches
  this one-time load; settings-page writes do not re-evaluate it.
- A config file that fails to parse prints `[pebrel:gpui] failed to read window
  config …` to stderr and falls back to the built-in canvas; the shell still
  opens.
- Issue #312 is only half closed: window size now follows config, position still
  does not.

## Validation

Local: `cargo fmt --all -- --check`,
`python3 scripts/check_architecture.py --base 7b0cedb` and
`python3 scripts/check_platform_cfg.py` (budget unchanged at 481) pass; no line
budget was raised, and `workspace.rs` is net −1 line because the grid math moved
to `config.rs`.

`lua_window_dimensions_reach_the_startup_grid` fails before the change, because
`StartupWindow` did not exist; it pins the Lua source → `dimensions()` link.
`startup_grid_follows_window_dimensions` pins the three shell-side verdicts:
unset → built-in canvas, configured → exact grid, below-minimum → clamped.
`startup_window_uses_the_config_file_selected_by_cli` covers CLI parsing through
startup loading. Follow-up builds and tests run through the upstream PR Actions.

## Supersedes

None.

## Revisit when

gpui exposes a window-position API or an app-level scale factor, and
`config.window.position` can be honoured with the same physical-pixel contract
the legacy shell has. Also when the GPUI shell moves onto a shared config
loader with the rest of the app; at that point `StartupWindow` should read from
it rather than re-discovering.
