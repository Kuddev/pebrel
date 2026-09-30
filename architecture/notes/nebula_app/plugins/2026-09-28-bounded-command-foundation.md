# Bounded native/Lua command foundation

## Status

Implemented command slice. It is not a resident UI host.

## Context

The user selected performance and memory as the primary constraints and rejected adding TS/JS,
browser or additional execution engines. Resource contributions should cost no plugin VM;
executable contributions can reuse the existing Lua 5.4 dependency.

## Evidence

- `Cargo.toml` already declares mlua with Lua 5.4 and serialization; the lockfile pins 0.11.6.
- `config/lua/runtime.rs` owns configuration evaluation, not persistent plugin state.
- `runtime_api` already owns commands, targets, outcomes and local discovery.
- The existing general CLI response reader accumulates a line without a consumer-specific size cap.
- `TerminalView::drop` closes its session, so UI replacement has a separate ownership requirement.
- mlua 0.11.6 documents memory limits and instruction-hook yields on managed Lua threads.

## Decision

Deliver one real public entry: `pebrel plugin check/run`. A bounded TOML manifest selects a native
Runtime request or a named Lua handler. A native request returns before any Lua initialization,
including in a mixed package. No GUI startup wiring, directory watcher, worker or global cache is added.

Lua initialization and the selected handler share an instruction/call/deadline budget. Native requests
yield out of Lua before reaching the existing Runtime. A command scope owns and drops the VM;
conversion has its own depth, text, node and allocation bounds rather than relying on the Lua limit.

The Runtime adapter reuses existing envelope and discovery types, caps frames, checks response
identity, preserves input-outcome uncertainty and bounds I/O by a cumulative deadline. The existing
general CLI and subscription readers are not rewritten as part of this consumer-specific boundary.
The shared response model preserves an explicit JSON null result, distinguishing it from an absent
result field without duplicating or reparsing the response envelope in this consumer.

## Rejected alternatives

- A new JS engine or browser: contrary to the chosen runtime and resource budget.
- Reusing the configuration VM: couples two independent lifetimes and expands existing APIs.
- Creating resident workers or registries before any consumer needs them: idle cost and unused state.
- Only a metadata checker: does not exercise an actual command through the existing public control plane.
- Wiring an unverified executor into paint or replacing TerminalView: risks input latency or session loss.

## Consequences

The first resource contribution is a declared native command, not a new theme/UI installation path.
Persistent activation, package management, events, multiple Lua modules and UI slots remain unimplemented.
The synchronous command adapter is explicitly not reusable as a GUI/worker callback; future resident
work must retain the bounded data/execution contracts while supplying an owned asynchronous driver.

## Validation

Windows GPUI product build and ten isolated public-CLI smoke cases passed, including native/Lua
dispatch, UTF-8 data, preserved error receipts, missing runtime, mismatched reply identity and oversized
reply rejection, plus an explicit null result. All 15 plugin regressions, 9 Runtime client regressions,
the public CLI parser regression and the existing configuration-Lua foundation regression passed
(26 focused Windows tests). The current tree passed the architecture checker and focused formatting.

No GUI throughput, process RSS, peak allocation or cross-platform result is claimed here.
Reported Lua memory is one VM's allocator usage only.

## Supersedes

None. The rejected exploratory TS proposal is not an accepted implementation decision.

## Revisit when

A real UI/event consumer is ready, a measured workload needs a revised budget, or plugin execution
requires stronger process isolation. No additional runtime follows automatically from those needs.
