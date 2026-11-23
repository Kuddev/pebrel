# Remote Files follows the focused SSH pane's reported directory

## Status

Proposed with the fix for #557; native validation is recorded in the PR.

## Context

The local Files view consumes the focused terminal's cwd reports. The remote view
instead probed the pooled SSH connection once and ignored later reports. Several
PTYs can share that connection, so the probe can correctly return `Ambiguous`
while each pane already has its own absolute cwd. The Files view then stays at `/`.

## Evidence

`TerminalView::remote_cwd` already validates SSH POSIX paths without inspecting the
host filesystem. OSC 7 and integrated shell titles emit `TitleChanged`, but the
workspace's sidebar synchronization previously returned early for remote browsing.
Restoring a cached remote listing also skipped checking a pane's newer reported cwd.

## Decision

Render routing, terminal events and open-drawer polling share the remote-browser
synchronization path. The focused, ready pane supplies its identity, destination
and reported cwd. A new binding uses that cwd before the connection-wide fallback.

Each pane snapshot retains its last observed shell cwd. Repeated reports preserve
manual browsing; an actual shell directory change follows the new remote directory.
A cached listing is reused when its pane's cwd has not changed, including between
panes using the same destination.

The browser owns one navigation task. A new navigation, binding change or browser
destruction drops the old task; `remote_call` closes its receiver and cancels the
network future. Generation and pane checks still reject late results. A cancelled
shell follow is observed again on returning to its pane; cancelled manual browsing
preserves the last confirmed listing. Failed targets remain available for explicit
Refresh without making every poll retry a failed request.

## Rejected alternatives

- Periodic connection-wide probes cannot distinguish multiple pooled PTYs.
- Interpreting remote paths through the host filesystem can show a local directory
  or reject a valid remote directory.
- Reloading on every title or pane activation loses manual browsing and selection.
- Keeping detached navigation tasks wastes obsolete work even with generation checks.

## Consequences

No persisted setting, protocol or authentication policy changes. Shells with no
cwd reports retain the existing unique-shell probe and root fallback; this fix does
not establish automatic following when remote shell integration is unavailable.

## Validation

`remote_files/follow_tests.rs` exercises real GPUI workspace subscriptions and redraws
with synthetic terminal cwd events and the real SFTP codec over owned duplex peers.
It covers opening, OSC 7/title updates, pane isolation, cached listings, manual
browsing, cancellation, failed-target refresh and request deduplication. It does not
authenticate to a real SSH server or inspect the user's credentials.

The fixture polls real network results on the GPUI test thread. A Tokio worker
cannot directly wake GPUI's deterministic scheduler; the test-only receiver uses
a noop I/O waker and a scoped pump instead of disabling that scheduler contract.
Production result delivery and cancellation use the existing bridge.

## Supersedes

None.

## Revisit when

SSH shell startup exposes an authenticated per-PTY cwd query for shells without
integration, or the SFTP browsing adapter changes its cancellation contract.
