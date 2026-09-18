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

## Scheduling and cost

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

This is a compatibility-focused delta pull implementation, **not** a raw PTY push
stream or a byte-credit protocol. A future push stream needs terminal-owned output
notifications, snapshot/delta ordering, acknowledgements after client consumption,
and bounded teardown/recovery tests; merely reducing the polling timer would not
provide those guarantees.
