# Plugin extension scope and terminal core ownership

## Status

Accepted responsibility boundary; the existing command entry enforces the first host-method boundary.
Wasm hosting, UI contributions, shader integration, market distribution and persisted authorization
are not implemented by this change. Validation results are recorded below after execution.

## Context

The user wants a broad plugin market for AI/terminal features, optional interfaces, Markdown and
editor extensions, appearance packs and performance strategies. Appearance is explicitly eligible
for plugins, including mouse trails and GLSL postprocessing shaders. This breadth must not make ordinary
terminal behavior depend on a plugin or let plugin teardown destroy a user's session.

The user separately requires product bugs to be fixed by product PRs, not by mandatory repair
plugins. A missing extension point needs an infrastructure PR; this is not a reason to exclude
the feature class from plugins. Resource use is reported for user decisions, while access remains
subject to explicit capability and scope authorization.

## Evidence

- `plugins/package.rs::allow_method` previously treated membership in package-controlled
  `permissions` as sufficient authorization, excluding only event subscriptions.
- `plugins/mod.rs::execute` and `plugins/lua.rs::drive` forwarded permitted strings to the Runtime
  adapter. Changing both the declared permission and command to `window.close` passed metadata
  checks; the existing missing-permission test covered only an undeclared call.
- `runtime_api/command.rs::RuntimeCommand::from_request` routes `window.close`, `pane.close`,
  input and execution methods to real application commands. A general automation API is therefore
  not an appropriate automatically exported plugin API.
- The existing examples use `runtime.describe`, which supplies service metadata without changing
  terminal state. Sensitive reads and control need further authorization semantics.
- `gpui_shell/terminal/view.rs` owns the terminal view lifecycle. Peripheral UI replacement must
  not be allowed to transfer or accidentally release that ownership.

## Decision

Make `plugins/BOUNDARIES.md` the authoritative responsibility contract. Product defects and core
behavior changes belong to product PRs; optional functionality and appearance belong to plugins;
new extension points are reviewed infrastructure changes. The contract distinguishes accepted
product scope from currently implemented APIs.

Keep host contracts and a small set of contract examples in the product repository. A separate
community repository may hold both its catalog and community-maintained packages; independently
maintained plugins can also be listed. Neither becomes part of the product's runtime dependency
graph. Creating or publishing a community repository is outside this boundary change.

Add one host-owned, exact method allowlist in `plugins/boundary.rs`, initially exposing only
`runtime.describe`. Check it during manifest validation and every native/Lua call. Unknown methods,
new Runtime methods, aliases and self-declared permissions do not expand access. Check reports the
host method set rather than presenting the general Runtime catalog as plugin permissions.

Do not edit the terminal core, Runtime automation API, input or rendering paths for this boundary.
Do not change the existing Lua budgets while the distinction between user-adjustable resource
policy and correctness limits remains unresolved. The future chosen Wasm runtime must enter
through host capability checks rather than inheriting the whole Runtime transport.

## Rejected alternatives

- Export every Runtime method named by a plugin: treats a request as an authorization grant.
- Block only known destructive method names: newly added methods and aliases silently gain access.
- Treat all reads as harmless: snapshots and terminal content can expose sensitive user data.
- Add a universal allow flag: reopens the exact core-control path this boundary closes.
- Switch execution engines first: execution isolation does not authorize host API calls.
- Freeze appearance or limit plugins to a few fixed forms: contrary to the accepted product scope.
- Repair existing terminal defects through optional plugins: makes base correctness depend on
  installation order and duplicates core rules.
- Build empty UI registries or lifecycle services now: no live consumer yet requires them.

## Consequences

Existing native and Lua overview examples continue to use their public CLI entry. Packages that
previously self-declared other Runtime methods now fail explicitly before transport. User-driven
Runtime CLI automation is unchanged. This is an intentional tightening of an unauthorised bridge,
not a claim that every approved plugin feature has been implemented.

The method check is not an OS sandbox, market approval, persisted user grant or protection against
arbitrary native-code compromise. GUI unload, focus recovery and shader failure contracts remain
requirements for their future actual integration points, not claims established by CLI tests.

## Validation

The Windows GPUI test target compiled and all 21 plugin tests passed, including six new regressions.
They cover self-granted core methods, unused forbidden permissions, unknown methods, native/Lua
dispatch rechecks, coroutine denial and recovery, and check/run entry behavior. The existing public
CLI parser test also passed, including its assertion that plugin commands do not select GUI startup.
The actual GPUI product configuration passed `cargo check --locked -p nebula --bin pebrel
--features gpui-shell`. Focused rustfmt checks and `git diff --check` passed.

The architecture checker with base `624a421a` reports the unrelated, already modified
`gpui_shell/workspace.rs` at 3482 lines against its 3481-line budget. This change does not edit that
file or relax its allowance. No GUI lifetime, input-latency, shader, marketplace or Wasm execution
validation is claimed by these command-boundary tests.

## Supersedes

The permissions-as-authorization interpretation of
[the command foundation](2026-09-28-bounded-command-foundation.md).
Its command-scoped execution, bounded data contracts and separation from configuration Lua remain.

## Revisit when

A concrete extension introduces host-owned scope grants and lifecycle semantics, or an engine/UI
integration needs a new boundary. Add capabilities intentionally with focused positive and negative
checks; do not infer them from the Runtime method catalog.
