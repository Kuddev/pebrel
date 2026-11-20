# Empty workspace residency

## Status

Proposed in the PR for #428.

## Context

Closing a tab means releasing its panes. With `keep_session` enabled, closing
that final tab should not also terminate the discoverable desktop process.

## Evidence

`finish_close_tab` and `close_settings` previously sent an empty workspace directly
to `close_empty_workspace_window`, which unregisters and removes the window.
The residency policy required live terminal panes. Separately,
`SessionPersistence::save_with` ignores empty checkpoints to protect saved
sessions; using a checkpoint after explicitly closing all tabs would retain the
old saved tabs.

## Decision

Route user tab/settings closure through the existing residency capability.
Ordinary empty workspaces may remain hidden when `keep_session` and the platform
visibility capability permit it. Preserve administrator isolation and the quick
terminal role boundary. Persist an empty workspace with `TabsClosed` before
hiding; keep the checkpoint reason for live workspaces.

Retain the empty window in the existing registry. Tray activation and mux attach
reveal it, and its existing plus controls and Ctrl+Shift+T create a fresh terminal.
Closing panes still sends shutdown and clears their registry/browser/bounds state.

## Rejected alternatives

- Creating a replacement terminal while hiding: starts a process the user did
  not request and undermines closing the final terminal.
- Persisting an empty checkpoint: may restore explicitly closed tabs.
- A second process-residency framework or a new setting: existing discovery,
  visibility and `keep_session` already own these responsibilities.

## Consequences

Explicit runtime window closure and empty sources after tab transfer still remove
the window. A failed durable save leaves the empty window visible with the
existing error feedback. A platform hide failure falls back to ordinary closure.

## Validation

The residency regression checks empty-workspace eligibility with retention on/off
and tray on/off. Existing persistence coverage verifies that `TabsClosed` clears
saved tabs through subsequent quit. GitHub Actions results are pending; native
tray/launcher visibility and shell release require platform acceptance.

## Supersedes

None.

## Revisit when

The desktop registry or resident discovery lifecycle no longer owns hidden windows.
