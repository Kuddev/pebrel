# Unobserved multiplexer clients keep outer screen evidence

## Status

Accepted on 2026-11-05.

## Context

An SSH pane can run a terminal multiplexer client in the foreground. When the
installed helper produces pane-scoped observations, those observations own the
remote Agent identity and screen samples. The 2026-11-02 active-pane
observation note in this directory made the view discard the outer title and
screen as soon as a remote target was known.

Two common setups never produce observations:

- The account that authenticated the SSH channel is not the account running the
  multiplexer server, for example a management login that starts the
  development session through `sudo -u`. Discovery only lists servers owned by
  the channel's account, and the server socket is private to its owner.
- The helper is unavailable: Agent hooks disabled, no `python3`, a timeout, a
  `disabled` marker, user-modified managed files or a symbolic link in the
  helper path.

In both cases an Agent inside the multiplexer was invisible. The outer program
recorded from the submitted command line is the multiplexer client or a
launcher script with an arbitrary name, and `screen_identity_allowed` only lets
shells and `ssh` relay a guest Agent's identity.

## Evidence

- [Foreground checkpoint](../../../../nebula_app/src/ssh_session/resume.rs):
  `has_client` already proves from the remote process list that a multiplexer
  client owns this PTY, independently of whether its server can be queried.
- [Discovery script](../../../../nebula_app/src/ssh_session/resume/discover.sh)
  filters processes by the channel's uid.
- [Outer evidence gates](../../../../nebula_app/src/gpui_shell/terminal/view/remote_activity.rs)
  and [screen identity](../../../../nebula_app/src/gpui_shell/terminal/view/notifications.rs).
- A restored Codex form is identified only when its model footer ends the
  screen ([manifest](../../../../nebula_app/src/agent_detection/codex.toml)); a
  multiplexer status line on the last row hides that anchor.

## Decision

- The monitor reports whether a multiplexer client owns the PTY
  (`SshEventHost::ssh_multiplexer_client`), including when its server is out of
  reach. An unreachable server keeps the last positively identified target, as
  before.
- Pane-scoped observations replace the outer title and screen only while one is
  current. A known target without a current observation no longer suppresses
  outer evidence.
- While a multiplexer client owns the PTY and no observation is current, the
  foreground program is treated as a relay: the screen may name the guest
  Agent even when the recorded program is a launcher name. Identity is retried
  without the first or last row, so a status line cannot hide an anchored form.
  Two Agents on one screen still identify nothing.
- No shell boundary is visible through the multiplexer. An Agent identified
  this way is released after it has been absent from the screen for four
  seconds; the previous program is restored and the command stays running.
- Once the client is gone or observations resume, the regular shell and
  observation lifecycles own the pane again.

## Rejected alternatives

- Matching launcher or multiplexer names in the view. Launchers have arbitrary
  names, and the remote process evidence already exists.
- Querying the other account's server through `sudo`. It would widen the
  helper's privileges and cannot be assumed to be non-interactive.
- Keeping the outer evidence suppressed whenever a target is known. That made
  every helper failure silently disable Agent identity.
- Clearing the whole foreground command when the screen-identified Agent
  leaves. The multiplexer client is still running and keeps its close guard.

## Consequences

- Split multiplexer windows rely on whole-screen rules without observations; a
  single Agent in one split can label the tab. Observations remain the precise
  path and win whenever they are current.
- Release is time-based screen evidence, not a process exit signal; a screen
  that hides the Agent form for four seconds releases it, and the next matching
  sample identifies it again.

## Validation

[View tests](../../../../nebula_app/src/gpui_shell/terminal/view/activity_tests.rs)
cover a launcher-started multiplexer client without observations (identity,
redraw tolerance, release and re-identification) and a known target without a
current observation. [Notification tests](../../../../nebula_app/src/gpui_shell/terminal/view/notifications.rs)
cover the relay rule.

## Supersedes

The outer-evidence rule of the 2026-11-02 active-pane observation note in this
directory, for panes without a current observation. Its observation, routing and
delivery decisions are unchanged.

## Revisit when

The helper can observe a server owned by another account, a multiplexer exposes
pane-scoped Agent state over a supported interface, or whole-screen identity
gains split-pane boundaries.
