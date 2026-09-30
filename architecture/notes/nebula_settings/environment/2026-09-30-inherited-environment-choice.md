# Choosing the environment for new Windows terminals

## Status

Proposed in the fix for issue #103.

## Context

Windows terminals refresh registered environment variables so newly installed
tools can become visible without restarting Pebrel. The same refresh replaces a
temporary PATH supplied by the shell that launched Pebrel. Both behaviors are
useful, but a registered PATH cannot reveal which inherited entries were temporary
and which came from an older registry snapshot.

## Evidence

`nebula_terminal/src/tty/windows/environment.rs` removes registered keys from the
inherited snapshot before applying current machine/user values. Its deletion and
expansion tests depend on that rule. Issue #103 supplies the concrete case of
prepending a directory to PATH in cmd before launching Pebrel.

## Decision

Add `refresh_environment` to the shared settings model, defaulting to true for
existing configurations. The Windows startup adapter, already used by both UI
shells, skips the registry refresh when the setting is false. The terminal then
inherits the environment of the Pebrel process, with normal pane-specific
overrides applied afterward. The setting affects newly created local terminals;
it does not rewrite an open process or a remote SSH environment.

## Rejected alternatives

- Always replace inherited PATH: loses deliberate launcher modifications.
- Always append inherited entries to refreshed PATH: can resurrect removed
  registered entries and change executable selection order.
- Guess temporary entries by comparing against a later registry snapshot:
  loses provenance when the registry changes before the first terminal opens.
- Disable refresh globally: changes the existing installation workflow.

## Consequences

Users can choose launch-time inheritance or current Windows settings. Missing or
invalid values keep the existing default. Reset restores refresh. The Terminal
settings row explains the difference and reports persistence failures through the
existing localized feedback path. No new dependency or core/UI dependency edge
is introduced.

## Validation

Settings tests cover defaults, invalid values, persistence round trips and reset.
A Windows subprocess regression prepends a temporary PATH entry and verifies the
real shell child receives it together with a pane override. A GPUI test clicks the
visible switch twice and verifies persisted state and search navigation.

## Supersedes

None.

## Revisit when

Windows supplies reliable environment provenance or the existing refresh policy
is replaced by an equivalent explicit per-profile configuration.
