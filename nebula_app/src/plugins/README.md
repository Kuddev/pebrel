# Plugin command foundation

This module implements an explicit, command-scoped plugin entry. It does not install packages,
keep plugins resident, register UI panels, or start a plugin worker during normal GUI startup.
The product adds no dependency or new language runtime for this entry.

## Public entry

From the repository root, using a newly built `pebrel` binary:

```text
pebrel plugin check nebula_app/examples/plugins/native-overview --pretty
pebrel plugin run nebula_app/examples/plugins/native-overview describe --pretty
pebrel plugin check nebula_app/examples/plugins/lua-overview --pretty
pebrel plugin run nebula_app/examples/plugins/lua-overview summary --pretty
```

`check` validates the UTF-8 TOML manifest, contribution rules, declared entry path and source-size
metadata. It does not read, compile or execute Lua. Its result explicitly identifies this scope.
`run` invokes exactly one declared command. `--args` accepts a JSON object overriding that
command's default `params`; `--timeout-ms` accepts 1 through 30000, defaulting to 3000.
Both commands output JSON and return a nonzero exit code on failure.

The example commands query `runtime.describe`, so execution requires a running Pebrel instance.
The check command is offline. The examples do not create panes or modify settings.

## Package and invocation contracts

- `plugin.toml` owns `manifest_version`, `api_version`, `id`, `name`, `version`, optional `entry`,
  `permissions`, and `commands`. Both numeric versions are currently 1; `version` is bounded
  package metadata, not a dependency-range expression. Unknown fields are rejected.
- Each command declares `id`, `title`, optional object `params`, and exactly one `method` or
  `handler`. A `method` directly invokes a declared Runtime method in Rust; a `handler` names
  a function in the table returned by `entry`. An entry exists exactly when Lua handlers exist.
- Native commands never construct a plugin Lua VM, including native commands in a mixed package.
  Only a selected Lua command reads its entry and creates a VM. The VM is dropped when that
  invocation returns, including errors. There is no shared module or VM cache.
- Entry paths are portable package-relative paths. Traversal, alternate roots and package-internal
  links/reparse points are rejected. The root itself is the directory explicitly selected by the user.
  These filesystem checks are not an OS sandbox against concurrent same-user directory replacement.
- The Lua entry is a single UTF-8 text file returning a command-function table. A handler receives
  `(ctx, args)` and returns one JSON-compatible value. Arbitrary module loading is not part of this
  initial entry; the existing configuration Lua module loader is unchanged.
- `ctx.runtime.call(method, params)` yields to the Rust driver. The manifest grants exact method
  names; `events.subscribe` is excluded because this entry handles one-shot replies. Runtime owns
  semantic argument validation, state and target identity. The discovery token remains in Rust.
- `ctx.null` represents JSON null. `ctx.array()` marks an empty array; nonempty arrays must have
  dense integer keys. Mixed, sparse, non-UTF-8 and unsupported values fail rather than lose data.
- Timeouts and errors do not roll back previously completed external actions. No request is
  automatically retried. Transport errors after submitting input retain the existing uncertainty
  description from the Runtime client. Unhandled Runtime errors retain their code and details,
  including partial-operation and deferred-cleanup receipts, across the Lua boundary.

## Resource and execution budgets

| Boundary | Current limit |
| --- | --- |
| Manifest | 32 KiB, 64 commands, 32 permissions |
| Lua source | 256 KiB |
| Lua allocator | 8 MiB per invocation |
| Lua instruction sampling | Yield every 10000 instructions; stop at 1000000 accounted instructions |
| Runtime calls | 16 per Lua invocation |
| Structured data | Depth 32, 4096 visitation/allocation units, 64 KiB cumulative key/string bytes |
| CLI argument JSON | 64 KiB before parsing |
| Runtime wire | 128 KiB request; 256 KiB complete response line |

The Lua memory limit is not a process RSS limit. File buffers, validated Rust values, interpreter
stack and existing application resources have separate costs. Conversion reserves output container
slots before allocation so nested/shared tables do not expand outside the structured-data budget.

Entry initialization and handlers both run as managed coroutines. The Rust driver regains control
through instruction yields, including loops wrapped in `pcall`. Filesystem loading, raw OS access,
unmanaged coroutines, dynamic Lua loading and direct output are not exposed to plugin code.
Instruction hooks do not preempt an arbitrary C operation; host I/O is outside Lua execution and
the one-shot Runtime transport applies a cumulative deadline to its connect/write/read loop.

This CLI driver performs that yielded Runtime request synchronously on the command process.
It is not an asynchronous worker adapter and must not be called from a GUI callback or a shared
resident worker. A persistent/event-driven host is a separate next slice, not implied by this CLI.

`execution.lua_memory_bytes` reports the VM's current allocator usage before it is dropped;
`instruction_ticks` is sampled VM work, not CPU cycles, peak memory or total process allocation.
The native branch reports zero Lua usage. Focused regressions live in `tests.rs`, with transport
and command-line integration assertions beside their existing owners.
