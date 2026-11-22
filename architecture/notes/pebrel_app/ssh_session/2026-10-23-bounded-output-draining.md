# Bounded draining of ready SSH output

## Status

Accepted for implementation; this narrows transport work, not a native-renderer
performance guarantee.

## Context

Local PTY reads already parse several available reads under a bounded terminal
lock before waking the UI. The SSH pump instead locked and woke once per SSH data
message. The UI mailbox coalesces notifications, but does not remove these lock
acquisitions or prevent a concurrent snapshot between already queued messages.
Remote redraws do not always carry synchronized-output markers.

## Evidence

On `fcb6971a`, the loopback fixture queues 100 synthetic history fragments and a
final screen in seven fragments, including split UTF-8 and control sequences.
`ssh_queued_unmarked_redraw_publishes_once_and_preserves_split_text` observes 107
Wakeup events. The new path consumes this available batch under one grid lock and
emits one Wakeup, retaining the same ordered terminal bytes and final screen.
This is a work-count comparison, not a claim of 107 rendered frames or a measured
end-to-end speedup on a user's network.

## Decision

Drain consecutive, immediately ready data and extended-data messages under one
terminal lock. Stop after 64 KiB, 128 messages, a non-data message, or a read that
would wait. Check budgets after each complete packet, so the byte budget can be
exceeded by the last packet. Do not concatenate or copy payloads into a new buffer.

Preserve the first non-data message in the existing pending deque and process it
on the normal pump path, including EOF and exit status. The nonblocking probe uses
the cancellation-safe channel receive; the next awaited receive registers its
normal task waker. There is no sleep, new task, additional dependency, frame-end
heuristic, or throughput limit. Work limits give competing input and grid readers
an opportunity between batches.

## Rejected alternatives

- Waiting for a quiet interval: delays ordinary interactive output and can hide
  continuous progress without identifying a reliable redraw boundary.
- Unbounded draining under one lock: can starve input and rendering during a
  large transfer or an empty-packet flood.
- Coalescing only UI notifications: already exists, but leaves per-packet locking
  and transport work in place.
- Inventing synchronization for unmarked output: the client cannot know whether
  another network packet belongs to the same frame.

## Consequences

Currently queued fragments have fewer lock handoffs and UI wakeups. Packets that
arrive later are still shown promptly; the implementation cannot guarantee atomic
display for an unmarked redraw spread over network gaps. Protocol markers retain
their independent parser buffering and watchdog. Packet contents and control
message order remain unchanged.

## Validation

SSH fixtures cover queued unmarked redraws, split Chinese text and escapes,
extended-data ordering, packet and byte limits, EOF/status ordering, regular
capability replies, resizing and disconnects. The separate synchronized-output
regression verifies gaps, input while buffering and silent-peer recovery. Native
Windows results belong to the corresponding PR; Linux execution alone does not
prove Windows visual smoothness.

## Supersedes

None.

## Revisit when

Measurements show the packet or byte budget holds the grid too long, the SSH
library changes receive cancellation semantics, or output processing moves to a
separate owner. Preserve bounded work and prompt normal output in any replacement.
