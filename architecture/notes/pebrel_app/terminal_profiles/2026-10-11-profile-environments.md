# Independent local session environments

## Status

Implemented; validation and review in progress.

## Context

Issue #546 asks for local shell sessions with independent environment variables.
The existing profile store deduplicates commands, so two sessions using the same
shell cannot retain different arguments or variables. GPUI also reduced profile
launches to shell identities, losing the local profile reference during splits.

## Evidence

`TerminalProfiles::upsert` matched executable paths. `TerminalView::new` created
`LaunchSession::Shell` for every local launch. ADR 0018 excludes arbitrary
environment maps and secrets from durable update/session recovery snapshots.

## Decision

Give custom sessions random stable identities and update saved rows by identity.
The local profile configuration owns explicit environment maps; invalid names,
NUL values, duplicate Windows key variants and integration-owned names are
rejected. Values are masked in the editor and omitted from diagnostic formatting.

Inherited environment, global configuration, then the profile overlay supply the
child environment. The existing Network terminal proxy, shell integration and
pane identity adapters retain their established authority. Empty values remain
present, and Unix keys remain case sensitive. Per-profile launch never mutates
the parent environment.

Running panes retain their profile identity and frozen overlay. Duplicate and
split create fresh processes from that snapshot. Local splits follow the current
pane directory rather than returning to a profile startup directory. The editor
keeps an independent argument presentation snapshot so duplicating an unchanged
profile preserves newline-containing and empty argv entries without borrowing
the original row identity used for compare-and-swap. The legacy default-profile
route loads the current saved profile and retains its full identity and overlay;
an explicit initial CLI command suppresses that unrelated profile selection. Durable snapshots serialize a
profile id and embedded command fallback, while the runtime overlay is skipped.
Cold restore resolves the current local profile. The editor explains that a
workspace export references local configuration and changes affect future launches.

## Rejected alternatives

- Process-global environment changes would leak between concurrent sessions.
- Command-path identities would merge sessions with distinct settings.
- Serializing all variables in workspace exports would violate ADR 0018.

## Consequences

Multiple saved profiles can share an executable. Import scans retain deterministic
command identities, and old v1 store/v4 session records load with empty overlays.
Saving compares the selected row under the store lock, preserving unrelated edits
and rejecting stale changes. The editor owns asynchronous tasks; destroyed views
cannot apply late results.

## Validation

Regression tests cover local store round trips, same-command identities, stale
writes, empty/case-sensitive variables, reserved keys and safe diagnostics/exports.
GPUI tests exercise actual editor controls and concurrent `/bin/sh` PTYs through
new-profile, split and duplicate paths. Platform-dependent execution must be
reported separately; Windows model coverage is not Windows native acceptance.

## Supersedes

None; preserves ADR 0018's durable snapshot boundary.

## Revisit when

Workspace portability gains an explicit secure profile transfer contract or
local profiles require an unset-variable operation.
