# Bind clipboard upload and SFTP negotiation to their owners

## Status

Implemented for the built-in SSH image-paste failure investigation.

## Context

Clipboard uploads reopened a destination by its saved alias, even though file
intake already held the pane's authenticated connection. Changed aliases or pool
state could therefore select a different transport. SFTP negotiation also sent a
subsystem request without inspecting the server's acceptance before starting INIT.

## Evidence

russh 0.62.2 `request_subsystem` only queues the request. Its byte-stream adapter
ignores channel Failure, so a rejected SFTP request waited for a protocol response
that would never arrive. Bare channels have no close-on-drop owner, unlike the
stream adapter. The loopback server regression exercises rejection and cancellation.
The user's original error text is unavailable; these tests do not establish that
their server used the same failure path.

## Decision

GPUI clipboard intake captures the existing pane connection and refuses a missing
owner. It uses the same pinned SFTP opener as file intake, without re-resolving the
host or triggering replacement authentication.
SFTP negotiation waits for subsystem Success, reports rejection/closure immediately,
and owns channel cleanup during every await. After acceptance, ChannelStream takes
over close-on-drop ownership before protocol INIT begins.

## Rejected alternatives

- Retry through an arbitrary fresh connection: may change the endpoint or require
  new authentication after the initiating pane is gone.
- Feed Failure to the SFTP byte stream: hides the cause behind a generic timeout.
- Disconnect the pooled SSH session: terminates unrelated interactive panes.

## Consequences

Existing file-transfer packet limits and authentication policy remain authoritative.
A server refusing additional channels or lacking SFTP still returns an error.
No new remote program, dependency, persistent setting, or font change is introduced.

## Validation

97 SSH tests passed, two existing opt-in tests skipped. Real loopback SSH/SFTP tests
cover repeated accepted sessions, immediate rejection, cancelled negotiation,
channel cleanup and preservation of the shared transport. User-machine image-paste
acceptance remains outstanding.

## Supersedes

None; extends pane-scoped upload ownership to clipboard images.

## Revisit when

An exact user error demonstrates a distinct server/channel-limit failure mode.
