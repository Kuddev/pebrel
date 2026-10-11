# Explicit local and SSH file tree pairing

## Status

Proposed for issue #219.

## Context

A local/SSH split can display only the focused pane's file tree. Focusing the
local source detaches the remote browser, so dragging between two trees in one
window is impossible. An extra source tree must preserve independent browsing
state without copying file operation or SFTP authority into the renderer.

## Evidence

`SidePanel` owns local/WSL enumeration, manual roots, expansion and search workers.
`RemoteBrowser` owns per-pane listings and navigation generations.
`RemoteTransferTarget` already captures pane, destination and directory for
pickers, drops and conflict confirmations, but cached browser identity alone
does not prove that its terminal pane is still present and connected.

## Decision

Default behavior remains the existing single drawer. An optional left source
drawer uses its own `SidePanel`, inputs, scroll and owned tasks. A user chooses
one local and one ready SSH pane in the current split. The only available pair
can be selected when enabling the mode; multiple pairs require an explicit
choice. The right browser follows that pair while focus changes between panes.

The source browser reuses shared traversal/path validation. Its file operation
scope is browsing, selection, search and drag; the default drawer remains the
existing file operation surface. WSL retains guest path and host UNC identities.

A pair stays bound to pane IDs and the captured destination. Pane closure,
replacement, disconnect or switching to a different split invalidates its live
scope and presents a choice instead of silently choosing another endpoint.
Transfers also check the live browser owner before capturing a request and before
starting a preflighted or confirmed transfer. Navigation generations continue to
reject stale results. Already accepted bulk transfers retain their captured target.

Both drawers keep independent preferred widths. Their combined width is limited
to 60% of the viewport after the tab sidebar; narrowing the window never changes
the preferences. Source tasks pause when hidden and disappear with their owner.

## Rejected alternatives

- Swapping the workspace's shared model while rendering: later GPUI callbacks
  would operate on another browser's state.
- Automatically selecting another connected SSH pane after disconnection:
  filenames can match across hosts while the intended upload authority differs.
- Adding a persisted pair format: pane IDs describe a running split, and the
  existing session restoration format does not promise stable browser identities.

## Consequences

The optional source tree and right browser remain independently browsable. VCS
and default local file operations retain their existing owner. This introduces
no production dependency, credentials fallback or automatic reconnect policy.
A test-only registered loopback transport bypasses personal SSH configuration;
its sentinel endpoints fail closed after the synthetic fixture is removed.

## Validation

Regression tests use actual GPUI controls and captured transfer targets. Owned
loopback SSH/SFTP fixtures use random server keys, temporary known-host files and
`auth_none` only, with no credential fallback. Native pointer/layout acceptance
and platform compilation remain necessary beyond virtual-window state tests.

## Supersedes

None.

## Revisit when

Product requirements need arbitrary many simultaneous remote trees or persisted
browser layouts; then browser instance ownership should become an explicit
workspace document model rather than another focused-pane heuristic.
