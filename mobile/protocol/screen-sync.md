# Bounded screen synchronization

The native PC terminal is currently a physical-cell mirror, not a second PTY.
Its palette, wide-cell boundaries, cursor and control interpretation remain owned
by the desktop terminal. No escape sequences received by the phone can execute
input, clipboard operations or OS commands.

## Negotiation and recovery

`mobile.ready.capabilities.screen_delta` advertises an optional extension to
`pane.read` with `screen: true`. Only supporting clients send `screen_since`.
The link removes that parameter before invoking the resident Runtime API.
Read-only authorization and explicit window/pane selection are unchanged.

The link stores one screen baseline per authenticated connection, never an
unbounded history. A response has `screen_seq` and either a complete `screen` or
`screen_delta: {base, rows: [[index, cells], ...], cursor, palette}`. A delta is
only used for the acknowledged baseline, matching pane and equal dimensions,
and only when it is smaller than a complete screen. Unchanged screens keep the
sequence and have no changed rows. The existing 40,000-cell, 128 KiB text and
2 MiB frame bounds remain in force. Resizing, switching panes, reconnecting or
losing a baseline requires a full snapshot. The phone rejects invalid bases,
duplicate/out-of-range rows and inconsistent sequences before replacing state.
Only an idempotent screen read may be repeated for recovery; input is never replayed.

## Change-driven streaming

`mobile.ready.capabilities.terminal_grid_stream` is advertised only when the
resident GPUI runtime supports `pane.screen.subscribe`. Subscribe with explicit
`window_id`, `pane_id`, and `lines` (1–100, default 100). The response returns a
`subscription_id`. Events carry the Runtime protocol/version envelope, that ID,
an independent consecutive `sequence`, and `data` in the screen/delta format
above. A subscription's first frame is always complete. Deltas may chain against
the previous sent frame because ordered delivery and all intervening consumption
are required; this differs from the one-outstanding-read legacy contract.

`pane.screen.ack` contains the explicit target, subscription ID and cumulative
consumed sequence. `pane.screen.unsubscribe` contains the same identity without
sequence. The mobile bridge rejects controls for another link's stream. Old
subscription events cannot mutate the newly selected pane. The phone applies
JSON deltas and decodes cells off the UI thread, publishes before ACK, and allows
four ACK receipts in flight so each display does not wait for a return trip.
Missing/invalid baselines trigger at most two read-only resubscriptions for a
fresh snapshot; no input is retried. Error and ten-second heartbeat events have
the same subscription identity. Unsubscribe releases the native watcher.

Terminal wakeups (including resize) and palette changes set a coalesced dirty
marker. The first eligible capture after idle is immediate; sustained captures
are limited to one per 33 ms and 2 MiB/s pacing. No changes means no captures or
`pane.read` polling. Native credit permits four frames and 256 KiB outstanding;
a larger recovery frame must wait until the window is empty and runs alone, still
within the 2 MiB frame limit. Admission failure preserves the last sent baseline
and one dirty marker, never a growing queue of stale grids. Sixty seconds without
progress while credit-blocked terminates that stream.

Input keeps wire order with up to eight outstanding RPCs, within the existing
16-RPC client budget; it does not wait one WAN RTT between keys. Completion is
reported only after all replies arrive. Failure or cancellation closes that
writer, cancels outstanding receipts and preserves unsent drafts. Input replies
omit redundant desktop snapshots; state remains on its separate subscription.

The native endpoint keeps its eight-slot request/reply channels. It reserves a
request slot before reading the next WebSocket message while continuing to drain
output. Runtime worker replies wait for bounded channel space; they do not abort
the connection merely because a burst filled that queue. No async network worker
blocks on a synchronous send, and dropping the receiver releases waiting writers.

Background cancels screen observation, not the remote PTY. Returning probes the
transport and reconnects previously successful LAN/relay profiles if necessary.
Retry delays back off from 500 ms to 15 s while foregrounded; trust/authentication
and protocol failures do not auto retry. New transport generations invalidate old
readers/writers while keeping the computer identity and draft. A different desktop
process cannot silently reuse the old selected pane ID.

## Legacy fallback scheduling and cost

The phone allows one screen request in flight. Input completion wakes that reader;
requests arriving during a read coalesce to one follow-up rather than cancelling
the read or building a backlog. Changed output is read again after 33 ms. Quiet
output backs off to 500 ms; user input interrupts that wait. Leaving the pane or
backgrounding the screen cancels the reader. Unchanged frames reuse the decoded
phone frame. RPC input remains ordered and separately permission-checked.

The desktop shell event queue now wakes its UI task instead of polling every
120 ms. It is bounded to 256 slots plus the channel's single sender reservation;
each dispatch consumes at most 64 events. Overloaded runtime commands get an
explicit error. This uses the existing futures dependency and no new thread.
Runtime JSON is serialized into one contiguous write, and the loopback and LAN
TCP paths disable Nagle buffering. This avoids per-cell tiny socket writes.

The fallback remains a compatibility-focused delta pull implementation. Only
negotiated clients use the change-driven grid stream described above. Both paths
share physical cells, colors, key authorization and bounded screen decoding.
