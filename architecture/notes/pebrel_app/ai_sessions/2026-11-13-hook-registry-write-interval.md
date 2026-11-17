# Hook session index rewrites are throttled to one per minute

## Status

Implemented.

## Context

AI clients emit a hook event per tool call, and nearly every event only refreshes
`last_seen` for an existing session. `record_hook_session` runs on the UI/event
path (`gpui_shell/terminal/view/agent_activity.rs` and
`window_context/agent_activity.rs`), and each event took the exclusive registry
lock, read and parsed the whole JSON index, updated/sorted/truncated the rows,
serialized it and replaced the file through `atomic_file::write` (`sync_all` plus
the write-through rename). Active AI turns therefore paid a file write per tool
call on the UI thread.

## Evidence

- One `record_hook_session` measured about 12.4 ms locally; reading and parsing
  the file costs about 0.05 ms on its own.
- Both production call sites pass `title: None`, so the only field that usually
  moves is `last_seen`.
- The registry is capped at 256 rows and is read by `scan()`, which uses
  `last_seen` as `modified` for ordering and the relative label. Claude and Codex
  rows also merge the native transcript mtime via `max`, so the registry timestamp
  mainly orders the remaining agents.

## Decision

After loading the registry, when the row for `(source, id)` already has the same
effective cwd and title and was persisted less than 60 s ago, return without
sorting, serializing or writing; the row keeps its previous `last_seen`. Empty
incoming cwd/title keep the stored value and never count as a change. New rows,
cwd/title changes and rows older than the interval still rewrite, with the id
validation, sort order, `REGISTRY_LIMIT` truncation and JSON format unchanged.

## Rejected alternatives

- Move the write to a background worker: the per-event fsync traffic remains, and
  a small JSON index would gain a thread lifecycle. Throttling removes most of the
  work with no new state.
- Keep writing every event but skip `sync_all`: weakens the durability contract of
  the only file that records resumable identities.
- Cache the registry in memory: several instances and helpers write the same file,
  so the cache would need cross-process invalidation.
- Never refresh `last_seen` while the identity is unchanged: ordering and relative
  labels would freeze at the first event of a turn.

## Consequences

- A session that keeps emitting events without cwd/title changes is written at
  most once per 60 s; ordering and "N minutes ago" labels stay minute-accurate.
- The row can remain up to 60 s older than the latest event, for every client.
- A wall-clock jump backwards makes `now.saturating_sub(last_seen)` zero, so that
  row is treated as just written until the clock catches up.
- Skipped events still take the exclusive lock file and parse the registry; the win
  comes from dropping serialization and fsync, not the read.

## Validation

`nebula_app/src/ai_sessions.rs` tests: `the_rewrite_interval_is_pure`,
`unchanged_events_within_a_minute_skip_the_rewrite` (an unknown-field sentinel
proves the file was not rewritten), `changed_cwd_or_title_still_rewrites`,
`events_after_the_interval_still_write` and `new_sessions_still_write`.
`scripts/check_architecture.py --base origin/main` passes.

## Supersedes

None.

## Revisit when

Hook events stop being the dominant writer, registry serialization becomes cheap,
or the resume list needs second-level freshness.
