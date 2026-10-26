# Retaining failed local sessions

## Status

Proposed for review with issue #512.

## Context

A removed WSL distribution still has an executable launcher. Creating its PTY
can succeed, then `wsl.exe` reports an error and exits. Closing every exited pane
discards that diagnostic and closes or hides the last window. A removed shell
executable instead fails at spawn and leaves a pane with no recovery control.

## Evidence

`TerminalView::process_event` receives child exit and PTY failure separately,
but both previously produced the same workspace close operation.
`try_restore_session` counted reconstructed tabs before asynchronous shell
startup completed and reported that count as a successful recovery.

## Decision

The terminal view owns the failure verdict. Retain a local nonzero child exit
when no input has been submitted and there is neither a current integrated
prompt nor a previously observed native CMD prompt. Retain local PTY failures
even after interaction. Successful exits and nonzero exits after interaction
keep their existing automatic close behavior. No elapsed-time threshold is used.

The workspace still records the process exit for Runtime API waiters; retaining
its view does not make the process live. The view keeps the terminal grid and
diagnostic text, and offers the existing shell picker and explicit close action.
Spawn errors expose the same recovery controls. Selecting another shell opens a
new tab; it does not rewrite the failed pane's saved launch or resend commands.

Restoration feedback describes reopened tabs and pending session startup, with
information severity. It makes no assertion that every asynchronous session has
started. Startup errors remain visible in the corresponding pane.

## Rejected alternatives

- Silently replace an explicit WSL or profile launch with a local default shell:
  this changes the execution environment and can replay a saved command in the
  wrong environment.
- Enumerate every shell or WSL distribution on the UI thread: this adds blocking
  work and cannot prevent deletion between enumeration and spawn.
- Treat a short process lifetime as failure: valid one-shot commands can exit
  immediately, while a failing WSL launch can take a long time.
- Retain every exit: ordinary `exit` and successful command completion should
  keep their existing behavior.

## Consequences

No persisted format, dependency or terminal protocol changes. A failing one-shot
command with no prior input also retains its diagnostic. A shell without prompt
integration that is externally terminated before any input is likewise retained.
The rule is conservative diagnostic retention, not a complete shell-readiness
protocol. SSH keeps its separate reconnect lifecycle.

## Validation

Workspace regressions exercise real terminal events, retained last-pane output,
Runtime exit records, normal exit, input/prompt counterexamples, PTY failure,
restored missing executables, actual recovery button hit targets and shell-picker
keyboard access. Windows WSL removal and native visual acceptance remain separate
from the deterministic GPUI tests.

## Supersedes

None.

## Revisit when

A shared shell-readiness protocol provides an explicit durable launch outcome,
or recovery UI gains an explicit in-place shell replacement operation with
well-defined cancellation and saved-command ownership.
