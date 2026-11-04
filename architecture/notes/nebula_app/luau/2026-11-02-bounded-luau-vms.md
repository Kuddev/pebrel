# Luau configuration and command VMs

## Status

Implemented for the requested Luau migration; native build validation is recorded with the change.

## Context

The owner selected Luau for plugins and `pebrel.luau` for configuration. Both callers
used one mlua dependency compiled for Lua 5.4. Enabling both backends is unsupported.
Lua 5.4 instruction hooks and its package loader also do not exist in Luau.

## Evidence

- [VM limits](../../../../nebula_app/src/luau.rs) own the common interrupt boundary.
- [Plugin driver](../../../../nebula_app/src/plugins/lua.rs) owns managed coroutines and host requests.
- [Configuration modules](../../../../nebula_app/src/config/lua/modules.rs) own local module resolution.
- mlua 0.11.6 documents that Luau interrupt yields are ignored across non-yieldable
  metamethod/C-call boundaries, while `catch_rust_panics(false)` propagates Rust
  unwinds through protected calls.

## Decision

Build mlua with vendored Luau and its optimizing bytecode compiler, without native
code generation. Each plugin invocation and config generation has its own VM.
Keep Runtime permissions, JSON quotas, package paths and native command routing
with their existing owners. An engine change grants no additional capabilities.

Count VM safe-point interrupts, not Lua 5.4 instructions. When a budget expires,
transport a private limit payload with `resume_unwind` and catch it at the Rust
entry into user code. Do not call the process panic hook or swallow unrelated
Rust panics. This prevents `pcall`/`xpcall` or a non-yieldable metamethod from
turning a catchable quota error into an endless loop. Host I/O retains its own
remaining deadline. VM interrupts cannot preempt arbitrary native operations.

Prefer `.luau` during discovery and creation, preserve `.lua` filename compatibility,
and never overwrite an existing config implicitly. Compatibility is Luau's language
subset, not all Lua 5.4 syntax or libraries. The configuration-only require loader
uses the entry directory, cached results, cycle detection and explicit source/count
budgets. It does not expose native packages or arbitrary filesystem traversal.
The shared source-budget checker also recognizes maintained `.luau` files.

## Rejected alternatives

- Two mlua backends: conflicting native `links` and duplicated runtime ownership.
- A plain interrupt error: protected calls can catch and retry it indefinitely.
- Yield-only cancellation: cannot stop all non-yieldable execution contexts.
- Leaving the old package loader: Luau has no Lua 5.4 `package.searchers` contract.
- Declaring a performance multiplier: no representative benchmark supports one.

## Consequences

Plugin execution reports `engine = luau` and `interrupt_count`. Existing package
manifests, host requests and JSON values retain their format. Config-only APIs stay
outside plugin VMs. Config generations and command failures release their VM and
cache; they do not close user sessions or create a resident plugin service.

## Validation

Formatting, source-budget/dependency checks and dependency resolution are performed
for the development commit. Existing assertions are adapted to Luau's quota names.
The user excluded regression execution; a package build is not native UI acceptance.

## Supersedes

The engine and instruction-sampling portions of
[bounded command foundation](../plugins/2026-09-28-bounded-command-foundation.md).
Its capability, JSON, filesystem and command-lifetime decisions remain applicable.

## Revisit when

Native code generation, resident plugin execution, module distribution or additional
host APIs are introduced. Each requires its own resource and capability review.
