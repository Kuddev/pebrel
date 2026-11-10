# Explicit safe restart

## Status

Implemented; Windows process-protocol checks passed. Product release and virtual-window execution are unverified because compiler processes exhausted available memory. macOS/Linux native behavior requires platform acceptance.

## Context

The advanced settings exit command must preserve the tray's document approval and session persistence. Restart must additionally reopen the current executable visibly without consuming a scheduled update or forcing session recovery.

## Evidence

- [Shutdown](../../../../nebula_app/src/gpui_shell/workspace/windowing/shutdown.rs) already owns draft approval, identity readiness, final snapshot checks and PTY shutdown.
- [Update handoff](../../../../nebula_app/src/update_download/handoff.rs) consumes scheduled installation at startup; update recovery tickets bypass ordinary restore preferences.
- [Restart integration harness](../../../../nebula_app/tests/restart_lifecycle.rs) exercises the actual replacement process without a GUI or user configuration.

## Decision

Use a short-lived replacement of the current executable, not a separate service. A loopback handshake with a random inherited token proves readiness before shutdown. Both peers have bounded readiness I/O. The prepared object owns cancellation until final authorization; dropping it kills/reaps only that uncommitted child, never the running application.

After readiness and acknowledgement, recheck drafts and identities and save through the existing shutdown capability. Only a successful save authorizes continuation. The replacement waits for the actual parent exit before CLI parsing, resident forwarding, updates or GPUI startup. Windows waits on an owned process handle; Unix observes direct-parent reparenting rather than guessing from a delay or a potentially reused PID. Poll intervals are observation cadence, not exit deadlines.

Restart preserves the explicit config path and inherited configuration environment, but not one-shot launch commands or login intent. A process-local authenticated restart flag prevents this startup from reading/consuming the scheduled installation or update restore ticket. Neither durable format nor user preference is changed. Later ordinary launches keep their existing behavior, including privileged-session isolation.

Blocking preparation and cancellation are background work. Final authorization is one nonblocking byte after the snapshot check; failure cancels the quit and preserves the live application. Readiness cannot guarantee every later OS, graphics or startup operation succeeds, so no early success message is shown.

## Rejected alternatives

- Start immediately and exit afterward: single-instance forwarding can reach the dying owner, losing the replacement.
- Fixed-delay relaunch or force-killing the old process: neither proves durable save or actual owner release.
- Reuse update installation handoff: it installs/replays update snapshots and overrides normal recovery preferences.
- Persistent restart flag/file: can suppress updates on unrelated future launches and needs crash cleanup.
- A general-purpose lifecycle framework or daemon: adds ownership without another consumer.

## Consequences

A replacement exists before exit but cannot create windows or terminals until authorized and its parent is gone. A handshake failure keeps the original application open. All exit paths still use the shared persistence rules. Pending installation and restore receipts remain untouched by normal restart.

## Validation

Windowless Windows harness: cancelled replacement does not start; authorized replacement starts only after owner exit; explicit config survives; token is removed; preparation failure returns to the owner. The test exposed inherited nonblocking mode on accepted Windows sockets; readiness switches accepted sockets back to blocking I/O before acknowledgement.

Product `cargo check` passed during implementation. GPUI tests are present but execution remains unverified on this machine; compile/check processes and the release dependency build hit allocation failures. Actual default focus, multi-window exit, restore switches, scheduled-update round-trip and macOS/Linux behavior remain acceptance work.

## Supersedes

None.

## Revisit when

GPUI provides an equivalent cancellable restart primitive that preserves executable/config identity and can explicitly bypass update installation for one launch, or a supported platform no longer reparents direct children on owner exit.
