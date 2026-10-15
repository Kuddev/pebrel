# Ended SSH pane retention

## Status

Proposed for review with issue #483.

## Context

An SSH server can close an idle shell using the same exit-status message as
an intentional logout. Keeping only a guessed timeout would require unreliable
inspection of terminal output or elapsed time. Unexpected disconnects without
an exit status already keep the pane and expose an explicit retry action.

## Evidence

The [SSH lifecycle](../../../../nebula_app/src/ssh_session/lifecycle.rs) treats
confirmed exit status as terminal exit and unexpected channel closure as failure.
The [loopback fixtures](../../../../nebula_app/src/ssh_session/lifecycle/tests.rs)
exercise both paths, including zero and nonzero statuses. Presentation previously
closed every exited pane in the workspace event subscriber.

## Decision

`RuntimeSettings::keep_ssh_session` defaults to false. It controls presentation
of completed built-in SSH panes at exit time, including sessions already open
when the setting changes. Local shell behavior and protocol exit semantics stay
unchanged. A retained pane remains exited to runtime clients, keeps its output
available, and offers a manual Reconnect button using the existing pane replacement
operation. Closing the retained pane remains an explicit action. Existing session
snapshots continue to preserve its launch identity and parameters. A cold start
with session restoration enabled starts a new SSH session according to that
existing setting; retention does not add a timer or automatic retry in the
running window.

## Rejected alternatives

- Inferring idle timeout from exit code, output, or duration cannot distinguish
  ordinary logout and would silently change behavior for some servers.
- Automatic retry can restart commands or prompt for credentials without user
  intent; retaining presentation must never grant permission to do that.
- A failed-connection overlay for a confirmed exit would hide the output that
  the setting is intended to preserve and misrepresent the protocol outcome.

## Consequences

Enabling the setting also retains intentional logout. The setting description
states this. Changing it does not reopen panes already removed or close retained
panes retroactively. Manual reconnect replaces the old pane and starts with a
fresh terminal buffer, using the existing destination, remote directory, custom
name, and split-position ownership rules. Retained panes participate in the
existing startup/update restoration contract, including configured AI recovery;
this is distinct from automatic retry after exit in the running window.

## Validation

The actual adjacent local-output regression exposed an unintended extra
ProgressChanged(None) before Exited: clearing retained SSH taskbar state had been
added to the common exit handler without a session-kind condition. The reset is
now scoped to SSH; the existing local title/progress/bell/exit assertion remains
unchanged, together with the new SSH exit/failure progress-reset assertions.
The original failure log is preserved and the targeted mutation restores only
that unconditional reset. Current rerun results are recorded separately.

Regression fixtures cover opt-in parsing, round trip and reset, real GPUI switch
persistence and rejected save, retained last-tab presentation, manual reconnect
routing, preserved recovery snapshot identity, and stale-entity isolation. The GPUI fixtures launch in child processes
with fresh synthetic application configuration and an explicitly empty SSH host
configuration source; their retry destination is rejected
before OpenSSH configuration or credential discovery. Separate loopback SSH
fixtures use synthetic keys and known-hosts files for real protocol exits and
unexpected disconnects. Native visual review and Windows server behavior remain
separate checks.

## Supersedes

None.

## Revisit when

A server-independent protocol outcome distinguishes idle timeout from logout,
or reconnect is redesigned to retain output across multiple connections.
