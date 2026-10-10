# Claude remote hook delivery and bounded continuation

## Status
Implemented and opened upstream for review; enabling it in an existing installation remains a separate approval.

## Context
Interactive Claude runs command hooks in a separate session without a controlling
terminal. The remote bridge wrote only to `/dev/tty`; ENXIO was swallowed by its
fail-open entry point. A sequence-state file proves invocation, not delivery.

## Evidence
- [Claude hooks reference](https://code.claude.com/docs/en/hooks#emit-terminal-notifications)
  documents the detached hook process and allowlisted `terminalSequence` output.
- An isolated Claude 2.1.287 streaming-truncation fixture produced `StopFailure`
  with `error=unknown` and `/dev/tty` errno 6. Its returned OSC 777 reached the
  interactive PTY without JSON appearing in the interface.
- A separate Pebrel dev instance displayed the failure notification and submitted
  exactly three `continue` prompts, with subsequent intervals over five seconds.

## Decision
- Fall back to stdout JSON only for Claude when opening `/dev/tty` fails with
  ENXIO, before any terminal bytes are written. Keep token, owner, sequence,
  payload bounds and other providers' existing transport unchanged.
- Settings own the default-on `ai_auto_continue` preference. The shared AI hook
  policy owns eligible error types, the three-attempt limit and cooldown; the
  GPUI pane owns the cancellable timer and PTY submission.
- Run only after existing owner/order checks. Claude `prompt_id` scopes late or
  duplicate turn events without preventing a session-end event for `/exit`.
- At dispatch, require the same session, idle failed turn, enabled setting,
  unchanged input epoch and live PTY. User input, a newer lifecycle event,
  disabling the setting or closing the session cancels pending work.
- Cancel on accepted input intent, not only when bytes reach the PTY. IME
  pre-edit can be abandoned, while clipboard confirmation, image staging and
  WSL path conversion can wait without sending bytes; none may leave a retry armed.
- Respect negotiated Enter encoding. Never broadcast automatic continuation.

## Rejected alternatives
- Treating a growing sequence file or a successful manual TTY probe as proof of
  delivery from an actual detached Claude hook.
- Writing to an ancestor's `/proc/.../fd/2`: bypasses the documented output path
  and introduces terminal-ownership assumptions.
- Unlimited retries, prose-based error matching, or retrying authentication and
  billing errors. These do not establish a recoverable failed turn.

## Consequences
`terminalSequence` is ignored by Claude in non-interactive `-p`/SDK mode; that
path is not claimed fixed. Existing Windows pipe delivery remains unchanged.
Retry stays independent of in-app toast preferences and does not hide failure.
The settings UI test must join the existing nextest shared-settings group.

## Validation
Remote bridge tests cover ENXIO, provider isolation, no duplicate output, payload
trimming and partial-frame cancellation. Policy/GPUI tests cover error allowlist,
limits, timing, duplicate/late events, settings changes, manual input and exit.
Input-intent regressions reproduce abandoned pre-edit and asynchronous image/path
intake leaving the timer alive, then protect cancellation before staging starts.
Native acceptance uses a local fake API and isolated configuration, with automatic
hook installation disabled so it does not replace users' hook files.

## Supersedes
None.

## Revisit when
Claude changes `terminalSequence`, `prompt_id`, or its error taxonomy; another
provider explicitly requests an equivalent retry contract; non-interactive
hook delivery becomes a separately accepted requirement.
