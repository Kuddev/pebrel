# Luau replaces Lua 5.4 for configuration and plugins

## Status

Implemented on Linux: library tests, the complete `pebrel` test binary and a native
GPUI window comparison passed. Windows and macOS builds, release package size and
user acceptance remain pending.

## Context

The user chose Luau as the plugin language and asked for one runtime shared by
configuration and plugins. Plugins have no real users yet; configuration does, so
existing `pebrel.lua` files must keep working without edits.

## Evidence

- mlua enables exactly one Lua version per build (`mlua-sys` rejects combined
  features with `compile_error!`) and declares `links = "lua"`, so Lua 5.4 and
  Luau cannot coexist in one process or one Cargo build graph.
- A probe of Luau 0.709 through mlua 0.11.6 showed: no `io`, `package`, `load`,
  `dofile`, `loadfile`, `warn`, `os.getenv`, `math.type`; `__pairs` ignored;
  `string.rep` drops its separator; `goto`, `<const>`, `<close>`, bitwise
  operators and hex floats are syntax errors; a call whose `(` starts a new line
  is a syntax error. `//`, `string.pack`, `utf8`, string coercion and `xpcall`
  arguments behave as in 5.4.
- mlua converts whole doubles to `Value::Integer` (i64 on 64-bit targets), so
  the existing TOML conversion keeps integer fields without changes.
- Public user configuration in issues #312 and #665 is plain data, profiles and
  modules; the generated templates use only `require` and tables.
- Luau has no instruction hook. Its interrupt runs at calls and loop back-edges and
  can yield only at yieldable points.

## Decision

Switch the workspace mlua feature from `lua54` + `vendored` to `luau`.

Configuration keeps the Lua 5.4 contract through a configuration-only layer:
`modules.rs` reimplements `package`/`require` (path templates, `LUA_PATH`,
`preload`, `searchers`, Lua 5.4 error text) plus `dofile`, `loadfile` and `load`;
`compat.rs` and `io.rs` restore the omitted standard library with Lua 5.4 return
conventions. `syntax.rs` blanks `<const>` in place, keeping positions, and turns
other 5.4-only syntax into line-numbered hints on the Luau error. Main chunks are
named `@path`, so errors read `path:line:`. Luau-style `./relative` requires and
`.luau` modules are added beside the old search order.

Plugins keep their restricted global set, now also removing `loadstring`,
`getfenv`/`setfenv`, `gcinfo` and `newproxy`. The instruction hook becomes a
Luau interrupt that yields every 10,000 checkpoints and stops at 1,000,000; once
exhausted it raises at every later checkpoint, so non-yieldable C boundaries and
`pcall` cannot hide it.

## Rejected alternatives

- Keep configuration on Lua 5.4 and run Luau plugins in a separate host process:
  feasible, but adds a second executable, a separate Cargo workspace and an IPC
  contract for a plugin system nobody uses yet. The user preferred one runtime.
- Plugins stay on Lua 5.4: abandons the chosen plugin language.
- Ask users to migrate their files: breaks working configurations for runtime
  internals they did not choose.
- Rewrite 5.4 syntax automatically (bitwise operators, `goto`): needs an
  expression parser and can change semantics; hints are safer. `<const>` is the
  exception because removing it never changes a valid program's behavior.
- Enable Luau `sandbox` for plugins in this change: it alters global and thread
  environment semantics; plugin hardening is a separate step.

## Consequences

Numbers are doubles: `math.type(1.0)` is `"integer"`, `tostring(3.0)` is `"3"`.
`goto`, labels, `<close>`, `_ENV`, bitwise operators and hex floats now fail with a
hint. `io` update modes, redirection and `popen` write mode are not restored;
`os.exit` raises instead of terminating Pebrel. Buffered reads and command output
are capped at 64 MiB. Plugin `instruction_ticks` now counts interrupt checkpoints.

Luau compiles C++ in every build; cross-check scripts already fake `CXX`.
`licenses/LICENSE-LUA` keeps its packaged name and now carries the Luau notice,
which includes the Lua.org copyright.

The GPUI product shell still reads terminal settings only from TOML; Lua decides
the startup grid and position there. That gap predates this change.

## Validation

- `config::lua` tests load a Lua 5.4-era configuration exercising modules,
  `package.path`, `preload`, `<const>`, `io`, `os`, `load`/`dofile`, `warn`,
  `math`/`string` gaps and `pairs` over the builder; others cover relative
  requires, missing-module text, syntax hints, file/command results and numbers.
- Plugin tests for budgets inside `pcall`, initialization loops, memory, payloads
  and removed globals pass on Luau.
- Full `pebrel` binary tests on Linux: 2245 passed; the one failure,
  `git_completion::tests::real_branches_cache_invalidation_and_directory_context`,
  fails identically on unmodified `main` on the same host.
- Under Xvfb, one configuration produced byte-identical screenshots from the
  Lua 5.4 `main` binary and the Luau binary; a variant computing its grid through
  `io.popen` resized the window; a bitwise-operator variant fell back to the
  default window and reported the `bit32.bor` hint.

## Supersedes

The Lua 5.4 runtime and instruction-hook budget in
[bounded command foundation](../../plugins/2026-09-28-bounded-command-foundation.md).

## Revisit when

Plugins move to a resident host, Luau `sandbox` or type checking is adopted, or
users report Lua 5.4 behavior that the compatibility layer does not cover.
