# Clipboard images follow the receiving command

## Status

Implemented; maintainer review pending.

## Context

Pebrel pastes a temporary PNG path into the active terminal input. Issue
[#981](https://github.com/Kuddev/pebrel/issues/981) reports that an open pane
stops accepting images after 32 pastes, even after the receiving commands end.

## Evidence

The original counters grew on every successful paste and reset only when the
pane was destroyed. The PTY confirms delivery of input, but cannot acknowledge
when a shell program or Agent reads a file.

## Decision

Derive the 128 MiB pane limit from retained attachments, without a lifetime
paste count. A command start assigns shell drafts to that command. Accepted
Agent prompt hooks assign Agent drafts to a turn; its completed turn can retire
those files if no background task is active. A new draft remains independent of
the previous turn. A fixed two-minute grace period separates verified
completion from cleanup. The pane owns the timer; local and SSH removal run
off the UI thread. Remote cleanup uses the same authenticated pane connection.
Files that cannot be removed stay in the quota while the original endpoint is
available; a failed removal is retried. An ended SSH connection releases
its quota without attempting cleanup on a new connection.

## Rejected alternatives

- Increasing the number of pastes only postpones the same failure.
- Deleting at paste or Enter can race a reader that has not opened the file.
- Reconnecting by host alias for cleanup could target a different machine.

## Consequences

A pane can paste more than 32 images. Files still waiting for a confirmed
completion continue to count against the pane's budget. A file path is
temporary; local files owned by the pane are also removed on close. Remote
cleanup is best effort while the original connection remains open.

## Validation

The [attachment tests](../../../../../../pebrel_app/src/gpui_shell/terminal/view/image_paste/lifetime.rs)
cover repeated pastes, byte recovery after the grace period, and separation of
shell, active turn, and draft files.

## Supersedes

None.

## Revisit when

Clipboard-capable clients expose a reliable acknowledgment that they have
consumed an attachment.
