# Plugin command foundation

This module implements an explicit, command-scoped plugin entry and a small install library.
It does not keep plugins resident, register UI panels, or start a plugin worker during normal
GUI startup. Installed plugins run only when the user invokes one of their commands.
The entry uses vendored Luauu through the shared mlua dependency, with a VM independent of configuration.

The division between optional plugins, product bug fixes and extension-point PRs is defined in
[BOUNDARIES.md](BOUNDARIES.md). Appearance extensions are explicitly in scope for plugins; this
command slice is not the implementation of that entire product scope.

## Public entry

From the repository root, using a newly built `pebrel` binary:

```text
pebrel plugin check nebula_app/examples/plugins/native-overview --pretty
pebrel plugin run nebula_app/examples/plugins/native-overview describe --pretty
pebrel plugin check nebula_app/examples/plugins/lua-overview --pretty
pebrel plugin run nebula_app/examples/plugins/lua-overview summary --pretty
```

`check` validates the UTF-8 TOML manifest, contribution rules, declared entry path and source-size
metadata. It does not read, compile or execute Luau. Its result explicitly identifies this scope.
It also validates requested methods against the host boundary and returns `host_runtime_methods`.
`run` invokes exactly one declared command. `--args` accepts a JSON object overriding that
command's default `params`; `--timeout-ms` accepts 1 through 30000, defaulting to 3000.
Both commands output JSON and return a nonzero exit code on failure.

The example commands query `runtime.describe`, so execution requires a running Pebrel instance.
The check command is offline. The examples do not create panes or modify settings.

## Install library and grants

```text
pebrel plugin install nebula_app/examples/plugins/theme-generator --grant theme-library
pebrel plugin list
pebrel plugin run example.theme-generator ocean
pebrel plugin disable example.theme-generator
pebrel plugin enable example.theme-generator
pebrel plugin uninstall example.theme-generator
```

- `install` copies only the files the manifest declares (`plugin.toml`, `entry`, `themes`) into
  `<settings>/plugins/<id>/`, then writes the host-owned `pebrel-install.json` receipt with the
  version, enabled state and grants. A package file with that name is never copied.
- A grant is a host capability beyond running commands. The manifest requests it implicitly;
  only the user grants it, with `--grant` or the Settings → Plugins confirmation. Installing fails
  before touching the library when a requested grant is missing, and grants that were not
  requested are not recorded. Reinstalling replaces the files and records the new answer.
- `theme_library` is currently the only grant. It lets the plugin add themes to the theme library;
  it never applies a theme or changes settings.
- `run` accepts an installed ID when no path of that name exists. Disabled plugins do not run.
  Running a package directory never carries grants.
- `uninstall` removes the plugin's files. Themes it added stay in the theme library.
- Settings → Plugins offers the same install, enable/disable, uninstall and theme generation.
  Its operations run on the background executor; results carry a sequence number so a newer
  operation discards an older one, and a closed page drops late results.

## Theme contributions

- `themes = ["themes/reef.pebrel-theme.zip"]` declares bundled theme packages (at most 16). They
  are validated with the ordinary theme package checks before the install becomes visible, and
  added to the library after install with a `metadata.package.origin` receipt whose `catalog` is
  `plugin:<id>`. Reinstalling the same version finds those receipts instead of adding duplicates.
- A command with `output = "theme"` must be a Luau handler. It returns either a complete theme
  document (with `schema_version`) or a spec `{ base = "<built-in theme>", ...overrides }`; a
  missing `base` uses the default built-in theme. The host deep-merges the overrides, records
  `metadata.based_on` and `metadata.generator`, and validates the result like any import.
- With the grant (an installed, enabled plugin) the theme is added to the library under a
  collision-free name. Without it, `run` returns the validated document only, for checking.
- Builds without the GPUI shell have no theme library; plugins that request the grant fail
  to install there.

## Package and invocation contracts

- `plugin.toml` owns `manifest_version`, `api_version`, `id`, `name`, `version`, optional `entry`,
  `permissions`, optional `themes`, and `commands`. Both numeric versions are currently 1; `version` is bounded
  package metadata, not a dependency-range expression. Unknown fields are rejected.
- Each command declares `id`, `title`, optional object `params`, optional `output`
  (`json` by default, or `theme`), and exactly one `method` or `handler`. A `method` directly invokes a declared Runtime method in Rust; a `handler` names
  a function in the table returned by `entry`. An entry exists exactly when Luau handlers exist.
- Native commands never construct a plugin Luau VM, including native commands in a mixed package.
  Only a selected Luau command reads its entry and creates a VM. The VM is dropped when that
  invocation returns, including errors. There is no shared module or VM cache.
- Entry paths are portable package-relative paths. Traversal, alternate roots and package-internal
  links/reparse points are rejected. The root itself is the directory explicitly selected by the user.
  These filesystem checks are not an OS sandbox against concurrent same-user directory replacement.
- The `.luau` entry (legacy `.lua` filenames are also accepted) is a single UTF-8 text file returning a command-function table. A handler receives
  `(ctx, args)` and returns one JSON-compatible value. Arbitrary module loading is not part of this
  entry; configuration has a separate directory-scoped module loader.
- `ctx.runtime.call(method, params)` yields to the Rust driver. The manifest requests exact method
  names; it does not grant access. Only methods explicitly exposed by `boundary.rs` are accepted
  at package validation and again at dispatch. Currently this is `runtime.describe` only.
  All other methods, including sensitive reads and core control, fail before Runtime transport;
  `events.subscribe` remains excluded because this entry handles one-shot replies. Runtime owns
  semantic argument validation, state and target identity. The discovery token remains in Rust.
- `ctx.null` represents JSON null. `ctx.array()` marks an empty array; nonempty arrays must have
  dense integer keys. Mixed, sparse, non-UTF-8 and unsupported values fail rather than lose data.
- Timeouts and errors do not roll back previously completed external actions. No request is
  automatically retried. Transport errors after submitting input retain the existing uncertainty
  description from the Runtime client. Unhandled Runtime errors retain their code and details,
  including partial-operation and deferred-cleanup receipts, across the Luau boundary.

This closes the earlier behavior where a package could name any one-shot Runtime method in its
own manifest and call it. New Runtime methods and aliases are not automatically plugin APIs.
The ordinary Runtime CLI is unchanged. A Runtime capability listed by `runtime.describe` is not
a plugin grant; `result.host_runtime_methods` in the CLI check output describes this entry's scope.
Future sensitive capabilities require host-owned authorization and target/lifecycle boundaries;
neither an installed package nor its own permissions list supplies that authorization today.

## Resource and execution budgets

| Boundary | Current limit |
| --- | --- |
| Manifest | 32 KiB, 64 commands, 32 permissions, 16 bundled themes |
| Bundled theme package copy | 48 MiB each; then the theme package limits apply |
| Luau source | 256 KiB |
| Luau allocator | 8 MiB per invocation |
| Luau safe-point interrupts | Stop at 100000 interrupts across entry and handler |
| Runtime calls | 16 per Luau invocation |
| Structured data | Depth 32, 4096 visitation/allocation units, 64 KiB cumulative key/string bytes |
| CLI argument JSON | 64 KiB before parsing |
| Runtime wire | 128 KiB request; 256 KiB complete response line |

The Luau memory limit is not a process RSS limit. File buffers, validated Rust values, interpreter
stack and existing application resources have separate costs. Conversion reserves output container
slots before allocation so nested/shared tables do not expand outside the structured-data budget.

Entry initialization and handlers both run as managed coroutines. A private quota unwind returns
to the Rust driver even from `pcall`, `xpcall`, and non-yieldable metamethods. Filesystem loading, raw OS access,
unmanaged coroutines, dynamic Luau loading and direct output are not exposed to plugin code.
VM interrupts do not preempt an arbitrary C operation; host I/O is outside Luau execution and
the one-shot Runtime transport applies a cumulative deadline to its connect/write/read loop.

This CLI driver performs that yielded Runtime request synchronously on the command process.
It is not an asynchronous worker adapter and must not be called from a GUI callback or a shared
resident worker. A persistent/event-driven host is a separate next slice, not implied by this CLI.
Settings → Plugins runs a theme command as one background-executor task per click; render and
input callbacks only start that task.

`execution.lua_memory_bytes` reports the VM's current allocator usage before it is dropped;
`engine` is `luau`; `interrupt_count` counts Luau safe points, not Lua 5.4 instructions or CPU cycles.
Luau uses its optimizing bytecode compiler; native code generation is not enabled. There is no
benchmark-derived speed claim. Lua 5.4-only syntax and libraries are not supported.
The native branch reports zero Luau usage. Focused regressions live in `tests.rs`, with transport
and command-line integration assertions beside their existing owners.
