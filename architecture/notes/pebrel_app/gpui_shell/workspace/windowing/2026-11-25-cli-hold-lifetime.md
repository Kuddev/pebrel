# CLI hold belongs to the launched pane

## Status

Proposed for review with issue #1098.

## Context

`--hold` is a documented terminal option, but the GPUI entry point discarded it.
An explicit `-e` command therefore lost its output when successful exit closed
the last pane. Elevated launches may be forwarded into an existing window, so
changing a process or window-wide setting would also retain unrelated panes.

## Evidence

`main.rs` did not pass `TerminalOptions::hold` into `run_shell`. Startup and
elevated handover carried the command and directory only. The workspace's exit
handler retained failed launches but closed every successful exit.

## Decision

Carry the option through initial launch and elevated handover into the created
terminal view. The view retains its grid on exit; the workspace still records
the Runtime exit event before deciding whether to keep the pane. Explicit close
continues through the existing close operation. Failure classification remains
separate: holding a successful command must not label it as a failed launch.

The option is ephemeral and is not added to saved sessions, settings, split
inheritance or new-tab defaults. A plain `--hold` starts one configured terminal
instead of restoring an unrelated saved workspace. Like explicit `-e`, it avoids
the ordinary resident Runtime handover, whose public API has no hold option.
Elevated handover preserves the flag and the existing window-selection policy.

The elevated request defaults missing `hold` to false and omits false on the
wire. Existing requests remain readable and ordinary launches retain their wire
shape. An older receiver rejects a true flag as an unknown field; it does not
silently acknowledge a launch whose requested lifetime it cannot honor.

## Rejected alternatives

- Retain all successful exits: ordinary shell `exit` would stop closing panes.
- Change command arguments to `cmd /k` or PowerShell `-NoExit`: arbitrary commands
  need not be shells, and changing argv is outside a window-lifetime option.
- Persist hold or make it global: later restored, split or unrelated resident
  panes would inherit a one-time CLI request.
- Reuse the startup-failure verdict: it would show failure controls for success.

## Consequences

No new dependency, worker, persisted format or public Runtime API is introduced.
The view flag is installed synchronously during pane creation before queued PTY
exit events can reach the UI subscription. Retained output consumes the existing
terminal grid until the user closes the pane.

## Validation

Regressions cover CLI-to-first-pane propagation, successful and nonzero exits,
retained painted output, Runtime exit records, explicit close, ordinary-pane
isolation, elevated window reuse, and old/new request serialization. Native test
execution and platform limitations are recorded in the PR validation results.
The architecture check against `76f74be1` passes without budget changes.
Restoring only the old exit predicate makes both held-exit regressions fail;
restoring this policy passes all 16 terminal-activity tests. A native Windows
GPUI probe retains a real short command's output with hold, closes normally
without hold, and exits after an explicit Runtime close-window request.

## Supersedes

None. Failed-launch retention remains governed by
`terminal/view/2026-10-24-failed-local-session-retention.md`.

## Revisit when

The public Runtime API gains an explicit launch-lifetime option, or session
restoration intentionally starts preserving one-time CLI lifetime policies.
