# Reuse the Windows Codex identity probe without startup feedback

## Status

Implemented; review pending.

## Context

[#673](https://github.com/Kuddev/pebrel/issues/673) reports Windows showing its
app-starting cursor during background Codex identity polls. Each poll launched a
fresh copy of the GUI executable. `CREATE_NO_WINDOW` suppresses a console; it does
not express the GUI startup-feedback policy. Known identities still need refresh
because a running Codex process can change its conversation.

## Evidence

The previous Windows `probe` always called `helper_command` followed by bounded
process output collection. The terminal caller polls missing identities every
two seconds and known identities every ten seconds. The stable Rust 1.97.1
`CommandExt::startupinfo_force_feedback` API is unavailable without a nightly
feature, so adding a creation flag to `hidden_command` cannot express this policy.
Windows defines `STARTF_FORCEOFFFEEDBACK` in `STARTUPINFO.dwFlags` instead.

## Decision

Keep one lazy worker per Pebrel process, serialized behind a mutex used only by
the existing background probes. Each request includes the shell PID and creation
time; the caller retains that process handle throughout the request. The existing
collector rechecks identities, examines only verified descendants, transports
only allowed metadata and never reads or seeks a duplicated live rollout handle.
It runs afresh for every request; this is not a cached conversation result.

Use private inherited pipes with one outstanding 12-byte request and a bounded,
length-prefixed response. Startup has a versioned readiness marker. A request's
existing two-second budget includes waiting for the worker and receiving its
reply. A failed or incomplete exchange discards the worker and channel; it is
not replayed. A later probe may start a replacement. Waiting for OS cleanup after
termination is separately bounded at 500ms.

The narrow native self-spawn sets `STARTF_FORCEOFFFEEDBACK`, retains console
suppression and supplies an explicit three-handle inheritance list. Arguments
are fixed internal selectors, not user commands. Create the process suspended,
attach the existing kill-on-close Job Object, then resume its owned initial
thread. The shared process-group adapter owns job creation and assignment for
both Rust-command and native spawns. Its job handle uses Rust `OwnedHandle`.

Helpers enter before GUI, hooks, configuration and normal CLI setup. The job
handle never enters the helper's inheritance list. Windows therefore reaps it
when Pebrel exits, including exit paths that do not run Rust destructors.

## Rejected alternatives

- Suppress console windows alone: does not control GUI startup cursor feedback.
- Change the global cursor or periodically restore it: can overwrite the cursor
  belonging to another application and leaves the process churn intact.
- Cache a discovered session forever: loses conversation changes in a live CLI.
- Inspect blocking native file APIs in a resident application thread: removes
  the existing ability to terminate a stalled probe.
- Change toolchains or add a generic process-launch framework: the fixed helper
  requires only a narrow native adapter using existing dependencies.
- Reuse a timed-out pipe or automatically replay the current request: permits
  stale replies to be associated with another pane or identity generation.

## Consequences

Healthy polls share one helper without launching a new process on each tick.
There is one bounded exchange at a time, no pipe reader thread, public endpoint,
discovery file, additional dependency or persisted setting. The process keeps its
idle helper until shutdown. WSL, Linux and macOS retain their existing probes;
terminal polling and stale-result rejection retain their existing authority.

## Validation

Native worker tests check the actual child's startup flags, repeated requests
using one PID, timeout/partial/oversized/failed replies, channel replacement,
bounded cleanup and parent exit without Rust destructors. Existing native
CMD/PowerShell fixtures verify separate shell ownership, current metadata,
ambiguous identities and unchanged live file cursors. Shared process-group and
output tests retain attachment, cancellation and output-bound coverage.

The opt-in cursor comparison samples Windows' real cursor while alternating the
previous one-shot probe and the reusable worker. It reports measurements rather
than imposing a CI threshold on a desktop shared with other applications.
On Windows 11 x64 (build 26300), twelve legacy requests produced twelve
app-starting transitions, twelve worker requests produced zero transitions using
one helper PID, and a second twelve-request legacy control again produced twelve
transitions. This exercises native test helpers, not a complete Codex GUI session
on the reporter's Windows 10 installation.

The built GPUI product's internal worker entry also served ten requests through
one process: repeated metadata reads, a rejected shell-creation-time mismatch and
changed metadata. Its pipe EOF exited cleanly before GUI startup; the legacy
one-shot entry still worked, extra arguments were rejected and the owned rollout
fixture's live file cursor stayed unchanged. Native CI and the PR report identify
the exact validation platform and any unexecuted acceptance checks.

## Supersedes

None. Extends the [owned process startup contract](../../2026-10-04-owned-process-startup.md)
with the same job authority and direct access to the native initial thread.

## Revisit when

Stable Rust exposes both startup-feedback and explicit handle-list control, or
Codex provides a reliable event interface covering conversation changes without
native handle inspection.
