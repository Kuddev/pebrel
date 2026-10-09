# Isolated conformance launch identity

## Status

Implemented; focused lifecycle tests and a native two-instance check pass.

## Context

The conformance harness promises an owned application and temporary configuration. Running it from a terminal inside an existing application also inherits that application's Runtime endpoint and pane identity.

## Evidence

An actual local launch exited successfully before opening its own Runtime endpoint and created the requested test-directory window in the resident application instead. Runtime state identified that one test window; it was closed without sending input to the user's original panes.

The endpoint resolver intentionally prefers an explicitly inherited Runtime endpoint over the default port file. Merely overriding the configuration directory therefore does not isolate a test launch. That resolver behavior remains correct for ordinary child CLI commands.

## Decision

Before spawning the isolated application, remove parent Runtime, process/pane and CLI identity, terminal identity, and explicit configuration-file overrides from the child environment. Then set both supported configuration-directory variables to the owned temporary directory. Keep unrelated environment such as PATH unchanged. The parent environment is never mutated.

## Rejected alternatives

- Change normal Runtime endpoint precedence: would break deliberate child CLI targeting.
- Clear the complete environment: loses toolchain, platform and caller test configuration.
- Kill an existing application or accept a successfully forwarded launch: violates the owned-process boundary.
- Add retries or extend startup timeouts: does not repair wrong-instance routing.

## Consequences

Existing Job Object/process-group containment and cleanup are unchanged. The additional work is a fixed set of environment removals at test launch, not a production runtime or per-frame cost. Regression failure output uses controlled values and does not dump the real parent environment.

## Validation

A new regression fails on the prior harness because the inherited Runtime endpoint reaches the child. After the change, that regression and all seven existing Windows lifecycle tests pass. A real packaged application launched with an inherited parent endpoint obtains a distinct Runtime process and the intended temporary working directory; after cleanup, the parent window and pane identities are unchanged.

This is process/configuration isolation evidence, not visual or frame-performance evidence. A locked desktop separately prevents meaningful native theme-transition capture.

## Supersedes

The assumption that a temporary configuration directory alone isolates an inherited application launch.

## Revisit when

The Runtime identity or supported configuration override contract adds new launch-routing fields.
