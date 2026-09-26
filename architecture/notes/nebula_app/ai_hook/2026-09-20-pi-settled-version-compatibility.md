# Pi settled-turn compatibility

## Status

Implemented while integrating PR #207.

## Context

Pi 0.80.4 added `agent_settled`, which follows retry completion. `agent_end`
alone may describe a failed attempt that Pi will retry. Earlier Pi releases
only expose the older completion event and package name.

PR #207 imported VERSION from the renamed SDK at module scope. Pi 0.60.0's
loader only aliases the earlier package name, so the extension failed to load
before its fallback could run. Mocking the import in event tests hid the failure.

## Evidence

The published older loader awaits extension factories and supplies SDK aliases
for Node installations or virtual modules for bundled installations. Loading
the original PR bridge through its alias rules produced MODULE_NOT_FOUND.
Guarded dynamic imports of both SDK names register successfully with both
alias and virtual-module modes using the actual Jiti loader.

## Decision

The asynchronous extension factory probes the available SDK through the host
loader. Missing/unparseable versions keep the existing agent_end fallback;
supported versions wait for agent_settled. Rejected event registration also
falls back. No unguarded runtime dependency on either package name is added.

Each attempt updates the result, but only the final settled edge sends it.
Session switches/shutdown discard unfinished results. The bridge sends typed
stop reasons without copying raw provider error text into helper arguments.

The shared notification model remains authoritative: failure and incomplete
results use issue notifications, cancellation is silent, and unknown results
remain unconfirmed. Legacy Pi events without metadata preserve their activity
edge but still produce a neutral result notification. Failures from the new
bridge cannot create a successful-completion activity badge. Existing pane
replacement, failure cooldown and user-selected duration remain in force.

## Rejected alternatives

- A static renamed-package import breaks old loaders before registration.
- Registering an unknown event cannot detect support: older event buses may
  accept the name without ever emitting it.
- A time-based debounce cannot prove that provider retries have settled.
- Replacing typed issues with localized strings would lose failure classification
  and prevent the shared cooldown/replacement policy from recognizing them.

## Consequences

Version probing occurs once at extension registration and adds no production
dependency or timer. Older/unknown hosts retain the limitations of agent_end;
they cannot acquire the newer provider event through a terminal-side heuristic.
Error details remain visible in the provider's terminal output.

## Validation

Bridge tests cover retries, cancellation, unknown outcomes, duplicate edges,
session switching and shutdown. A loader test erases TypeScript only and checks
real module resolution with old/new/missing/invalid SDK metadata. Local Jiti
probes cover Node aliases and bundled virtual modules for Pi 0.60.0 and 0.85.1.
Rust regressions cover typed notification and activity behavior, including
background work and legacy payloads.

## Supersedes

None.

## Revisit when

Pi exposes a version-independent capability API, or the supported minimum
release guarantees agent_settled and no longer requires the older package alias.
