# remote multiplexer history belongs to the attached client

## Status
Accepted for implementation.

## Context
Alternate-screen touch scrolling fell back to arrow-key sequences without mouse
tracking. In remote multiplexer this can move shell input instead of reading history. Merely
focusing the IME did not restore the live viewport or leave remote multiplexer copy mode.

## Evidence
`TerminalSession.wheel` delegated every alternate-screen gesture to the VT wheel
encoder. `GhosttyView` only requested IME focus on a tap.

## Decision
For app-launched remote multiplexer attachments, capture the PTY from a nonce-scoped startup
marker, remove only that marker from the byte stream, and resolve the specific
remote multiplexer client by its tty. Do not guess from the first server session or display title.
Track that client's session/window/pane for reconnection, including periodic
checkpoints and a checkpoint when the activity backgrounds.

Typed history commands run through the existing independent SSH query channel.
Copy-mode scrolling, exit from copy mode and subsequent terminal input share
one ordered IO queue. An input action discards older queued fling events. Tapping
to open the keyboard restores input before IME composition begins. Hide the live
input cursor while reading history. History errors reject input instead of closing
the SSH transport or blindly sending escape/cancel characters to a process.

## Rejected alternatives
Arrow-key emulation changes program input. Sending Ctrl+B or q assumes user remote multiplexer
bindings and can enter the shell. A global mouse setting changes other clients.
Inferring a client from session titles can act on another active task.

## Consequences
Generic terminals retain VT wheel negotiation. The typed remote multiplexer controller applies
to remote multiplexer connections created/selected by the app, not arbitrary nested remote multiplexer started
inside an opaque shell. Checkpoints preserve the last observed target, not an
atomic server-side record of every change made immediately before network loss.

## Validation
JVM tests cover byte-exact startup-marker filtering, client selection, copy-mode
commands, current-window tracking, IME focus restoration and repository recovery.
An isolated real remote multiplexer server confirmed client tty fields, pane targeting, scrolling,
copy-mode cancellation and window tracking. Android device acceptance is separate.

## Supersedes
Extends the target preservation in `../session/2026-10-22-inline-ssh-recovery.md`.

## Revisit when
Adding a server-side session protocol or supporting opaque nested multiplexers.
