# Explicit authorization for background effects

## Status

Implemented in the settings contract and the private native qualification consumer.
The application renderer has not activated these effects. Public templates must
not advertise product execution until the native capability is qualified.

## Context

A shader path in a shared theme or a disabled local configuration must not grant
execution. The user requested configuration before UI controls, with every
decorative effect disabled by default, WGSL as the sole user and builtin shader language.

## Evidence

The existing settings parser supports explicit boolean values and preserves
unknown preferences. Naga validates WGSL and emits native backend
languages. A tiny loop-heavy fragment validates successfully; source size and IR
validation therefore do not establish a GPU execution-time bound.

## Decision

Keep local authorization in the dependency-free settings authority. Separate
enabled values from optional source paths. A path alone cannot enable execution.
Explicitly enabled WGSL takes precedence over the controlled grain effect.
Retired prototype language keys have no execution authority.

Rendering capability belongs to the consumer. This model performs no disk reads,
shader compilation, source watching or playback scheduling. Theme package
metadata remains unable to authorize shader execution or install unsupported
media kinds. Reset removes authorization and paths without removing user files.

The initial qualification consumer admits one static fragment, one color output
and a position input. It supports named WGSL entry points. No compute, resource
bindings, feedback targets or includes are admitted. It compiles only on explicit
startup; edits to a source require another explicit start, without a resident
watcher. This is a qualification boundary, not a final shader feature specification.

## Rejected alternatives

- Inferring trust from a filename, package resource kind or a nonempty path.
- Exposing multiple user languages and maintaining competing source contracts.
- Placing parser/compiler/renderer dependencies in the settings crate.
- Advertising settings as functional product switches before native acceptance.
- Treating compiler admission as a bound on arbitrary GPU execution time.

## Consequences

Old and disabled settings preserve existing behavior. Opt-in parsing allocates
only ordinary cold configuration data. Runtime consumers must expose unsupported
capability/error feedback and retain static fallback. The qualification compiler
uses a finite worker process; native GPU ownership and platform acceptance remain
separate responsibilities.

## Validation

The actual settings test binary passes88 tests, including explicit false,
invalid/old values, path-only inputs, roundtrip, missing-source errors and retired-key isolation
and reset. The compiler passes10 tests for WGSL parsing, named entry points,
output languages, unsupported ABI and the loop-heavy counterexample. Native
rendering results belong to their executed acceptance report, not these tests.

## Supersedes

None.

## Revisit when

Native platform acceptance permits application activation, input bindings/time
are measured, or sharing/exporting shaders requires a distinct trust contract.
