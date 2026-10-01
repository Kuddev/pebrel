# Installed notes wait for a visible window

## Status

Implemented; regression validation pending.

## Context

The post-install notice is persistent and shown once. A resident first launch can have only a hidden workspace.

## Evidence

The original UpdateInstalled dispatch chose an MRU window without inspecting window_hidden, then persisted release_notes_shown before opening its dialog. Quitting before revealing that hidden window could consume a notice the user never saw.

## Decision

The update dialog capability owns one pending installed description in App memory. The process shell event dispatcher chooses a live visible workspace before claiming and opening it. Existing pump ticks retry after reveal; a destroyed handle leaves it pending. Failed persistent claims retry after five seconds. No activation or forced reveal is added.

The complete process shell event dispatcher lives under windowing/shell_events, separately from window creation, transfer, geometry and snapshot responsibilities. Its existing event behavior is preserved.

## Rejected alternatives

- A hidden dialog must not count as a visible notice.
- Forcing a background resident window to the foreground would change startup policy.
- A second persisted notice state would duplicate the existing locked updater authority.

## Consequences

Only a successful persistent claim for a visible target consumes the pending notice. Process exit before reveal preserves the existing unclaimed update state. The pending payload is App-owned and does not create another worker or persistence format.

## Validation

An isolated GPUI regression sends the real installed event while the workspace is hidden, checks that no persistent acknowledgement exists, reveals it, and checks one acknowledgement. Existing rendered-dialog tests cover content and close/Escape.

## Supersedes

None; refines the installed release-notes lifecycle.

## Revisit when

Resident window visibility or updater acknowledgement ownership changes.
