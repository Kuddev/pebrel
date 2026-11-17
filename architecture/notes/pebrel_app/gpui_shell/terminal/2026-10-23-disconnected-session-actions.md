# Keep disconnected SSH output readable

## Status

Proposed for review.

## Context

The connection card belongs to startup, when the terminal is empty. Previously,
an SSH failure after Ready recreated that card over the existing terminal. This
hid the user's last output and made a dropped connection look like initial setup.

## Evidence

The connection presentation cleared the card on Ready, then created a new one
for any subsequent stage when no card existed. A Failed notification therefore
covered the retained terminal model, even though the shell lifecycle deliberately
keeps that model after an unexpected disconnect.

## Decision

Keep the startup card for initial progress and startup failures. Once Ready has
removed it, a later failure renders a small terminal-local disconnect notice with
reconnect and close actions. Repeated failure notifications keep the card hidden.
The notice does not inject text into the remote terminal model or resize its grid.

The view derives the disconnected presentation from the existing SSH stage and
card state. It does not add a second connection state machine. A local pending flag
deduplicates retry requests. Both retry surfaces use the existing workspace event,
which replaces the failed entity and isolates late events from the old connection.

The plain R key is a local action only in the disconnected presentation. Native
shortcuts, modified keys, held repeats, unfocused terminal input, IME composition,
text commits and paste do not trigger it. Normal connected R remains terminal
input. Failed SSH panes neither write nor broadcast input, retaining selection and
scroll position when the user types into a disconnected pane.

## Rejected alternatives

- Reopening the full startup card hides useful output after a disconnect.
- Writing a reconnect prompt into the terminal parser alters remote output,
  selection and scrollback, especially in an alternate screen.
- Restarting transport inside the existing view duplicates the workspace's retry
  lifecycle and allows stale session results to affect a replacement connection.

## Consequences

The existing output remains readable and selectable until the user reconnects or
closes the pane. Reconnecting replaces the pane using the existing startup flow;
this is not automatic reconnection or restoration of a remote process. No new
thread, network action, dependency or persistent format is introduced.

## Validation

Real GPUI tests cover connected R, disconnected keyboard and button activation,
duplicate requests, modifiers, IME input, initial failure, unchanged grid and scroll
position, selecting and copying output, and button bounds at 360/900-pixel widths.
The SSH-filtered application suite passes 258 tests with three existing fixture or
native-platform exclusions. The i18n contract passes 24 tests with one existing
informational benchmark ignored. Windows visual acceptance remains separate.

## Supersedes

None.

## Revisit when

Automatic reconnection or preserving the same model across a replacement session
becomes an explicit requirement. Such a change needs a session-generation and
cancellation contract beyond this presentation change.
