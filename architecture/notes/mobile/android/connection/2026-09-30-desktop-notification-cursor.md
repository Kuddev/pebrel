# Keep desktop notification cursors across reconnects

## Status
Accepted for implementation.

## Context
Creating a notification reducer for every socket loses the last observed task
sequence. A reconnect then treats the latest state as its first snapshot and
misses a completion that happened during the interruption.

## Evidence
Desktop snapshots expose a process identity and per-pane state-change sequence.
They contain the latest state, not a durable log of every intermediate event.
The repository already rejects callbacks from replaced connections.

## Decision
The computer owns its reducer. A saved relay also checkpoints at most 512 pane
identities and sequence numbers in existing private preferences, only when the
cursor changes. No terminal text or command is persisted by this cursor.
Observe snapshots after the current-client check on the main owner. Keep advancing
the cursor when desktop notification policy is disabled. A different process
establishes a new baseline. Forgetting the computer removes its saved cursor.
Notifications carry process identity into existing pane navigation guards and
describe finished, failed, waiting-input and attention states separately.

## Rejected alternatives
A per-socket reducer misses changes during reconnects. Replaying every state
would require a server event log that this protocol does not provide. Matching
only pane IDs can open a different pane after the desktop process restarts.

## Consequences
Reconnect can report the latest observable transition for an already known pane;
intermediate transitions and never-observed panes cannot be recovered. An async
preference write is not an exactly-once delivery guarantee across abrupt death.
Malformed checkpoints reset to a quiet initial snapshot.

## Validation
The existing notification reducer test covers checkpoint restoration, duplicates,
muted transitions, process replacement and malformed saved data. Notification
interaction on physical devices remains unverified.

## Supersedes
None.

## Revisit when
The desktop provides stable process-instance IDs or a durable event replay API.
