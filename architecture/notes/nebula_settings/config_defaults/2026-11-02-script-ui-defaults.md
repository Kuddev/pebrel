# Startup script defaults for native settings

## Status

Implemented for the `pebrel.luau` configuration entry.

## Context

GPUI used the shared line-based settings model, a separate sparse TOML adapter,
and a shared script parser for startup window geometry. Evaluating a script from
each settings getter would put executable code and repeated I/O in UI callbacks.
Applying values only to GPUI's Settings global would leave wallpaper, theme and
settings controls reading different values.

## Evidence

- [Defaults boundary](../../../../nebula_settings/src/config_defaults.rs)
- [Script field adapter](../../../../nebula_app/src/config/desktop.rs)
- [GPUI startup owner](../../../../nebula_app/src/gpui_shell/config/source.rs)
- [Settings and reset semantics](../../../../nebula_settings/src/lib.rs)
- [User configuration contract](../../../../docs/lua-configuration.md)

## Decision

The application parses its entry before the GPUI event loop and background
consumers start. Install an immutable map of plain setting defaults in the shared
settings crate, which remains free of production dependencies, language parsing,
filesystem watchers and application services. `RuntimeSettings::from_raw` keeps
ownership of valid ranges, defaults, enum aliases and theme inheritance.

Saved UI preferences take priority over script values, preserving the existing
settings-file authority. Explicit empty values keep their clearing semantics.
Removing a saved override reveals the script default; resetting known settings
does the same. No save operation rewrites arbitrary user code. Script changes take
effect on restart. The parsed font/color/mouse presentation subset is cached, so
later Settings saves do not rerun the script. Window geometry retains its separate
startup lifetime.

## Rejected alternatives

- Executing Luau in `RawSettings::load`: repeated work and UI-thread stalls.
- Only overriding one GPUI global: wallpaper and controls would disagree.
- Giving script fields unconditional priority: existing controls would appear to
  save successfully and then silently revert to script values.
- Editing return statements in user scripts: unsafe for arbitrary control flow,
  imports and user comments.
- Adding a second typed settings model: divergent clamps and default rules.

## Consequences

`config.ui` is a defaults layer, not a second settings persistence service.
An absent config retains built-in behavior. Invalid script loading reports a
notice and starts with saved/built-in preferences. Existing TOML presentation
retains sparse-field precedence. Native script hot reload is not claimed.

## Validation

Formatting, architecture and diff checks accompany the commit. Windows compilation
is performed for the package. Regression execution was excluded by the user;
compilation does not establish native interaction acceptance.

## Supersedes

None. This extends the shared settings reader while preserving saved-key authority.

## Revisit when

Native script hot reload is requested. It needs a serialized worker and atomic
generation publication across all consumers, rather than mutable per-view defaults.
