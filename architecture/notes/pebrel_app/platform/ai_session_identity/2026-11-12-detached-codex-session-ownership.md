# Detached Codex processes cannot own a terminal session

## Status

Proposed for [#633](https://github.com/Kuddev/pebrel/issues/633).

## Context

A shared backend may inherit a pane's environment and then open another pane's
rollout. It can also run that conversation's hooks. Saving either signal under
the inherited pane can make Pebrel restore the other project's conversation.

## Evidence

In an offline Linux fixture with two real 0.162.1 TUIs, the independent client
held its own rollout. The shared backend inherited pane A's environment and
held pane B's rollout. That backend had a separate process session with no
controlling terminal. The upstream hook runner also creates a new process
session for its helper, including when the launcher is an independent TUI.
The base hook socket emitted the shared backend's `SessionStart` with pane
A's ID when the conversation came from pane B.

## Decision

The Linux rollout probe requires the candidate to share the pane shell's
kernel session and controlling terminal. The WSL guest probe excludes
processes without a controlling terminal before reading their open files.
Both keep the existing pane, instance, metadata and ambiguity checks.

On the Linux hook socket, use the kernel peer PID to find the closest live
Codex ancestor before acknowledging the helper. If that ancestor has no
controlling terminal, acknowledge the helper without routing its event. The
search stops at the closest ancestor so a detached backend cannot borrow
the terminal of the client that launched it.

## Rejected alternatives

- Matching working directories cannot distinguish two panes in one project.
- Accepting `source=vscode` describes the rollout's origin, but cannot prove
  which pane owns it. It would make the detached backend's files eligible.
- Checking only the hook helper's terminal would discard real direct TUI
  events. Checking only `--managed-daemon` misses pinned backends that omit it.

## Consequences

The detached shared backend cannot supply new identities through these
paths. A shared client's conversation stays unidentified unless Pebrel has
another verified identity. Native Windows and macOS ownership, and existing
saved identities, need separate evidence and handling.

## Validation

- Linux native and guest probe tests combine a real controlling PTY with a
  detached process holding a second valid rollout under the same pane and
  instance. The owned rollout stays identifiable; a detached-only match is
  ignored. The Unix socket test runs detached helpers from both attached and
  detached launchers and checks acknowledgement and routing.
- Two real 0.162.1 TUIs in an offline Linux namespace retained their separate
  state. The independent TUI's native `SessionStart` reached the correct pane.
  The shared backend's helper received an acknowledgement without delivering
  an event, and its rollout was not attributed to either pane.

## Supersedes

None.

## Revisit when

The shared protocol exposes a verifiable link between a specific terminal
client and its conversation, or a supported client owns its rollout outside
the pane's controlling terminal.
