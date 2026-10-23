# Bounded output storage and grid anchors

## Status

Implemented as the terminal transport/capture boundary. Native renderers and the
CLI producer are separate consumers; this change does not advertise their
capability or replace ordinary output with widgets.

## Context

An inline renderer needs output ownership and positions that survive terminal
scrolling. Absolute row numbers become stale under reflow. Ordinary PTY content
is display state, not lossless stdout, and GUI code cannot safely reinterpret
every interactive program as a document.

## Evidence

`StreamProcessor` already commits VT and recognized OSC events in wire order.
`CellExtra` holds optional uncommon metadata. Shell markers provide command
boundaries, while the existing literal command parser rejects pipelines and
expansions. The existing reading epoch identifies resets/reflow that invalidate
captured source ranges.

## Decision

Keep the versioned private `OSC 777;pebrel-view;1` packets in terminal code. They
carry Begin/Place/Append/End records and preserve stdout/stderr identity. This
private extension is not presented as a universal terminal standard. MIME data
is opaque to the core.

Bind each reserved row's first cell to a shared source through optional metadata.
The sender supplies real line breaks, so the PTY and frontend advance together.
Only contiguous column-zero placement is accepted. Commit pending synchronized
output before placement. Ordinary cells retain their existing size.

Frame, source and retained-store limits bound storage. The registry holds weak
references; clearing the last marker releases its source. Eviction clears bytes
and exposes an explicit status on any surviving markers. Incremental updates
clone only new shared chunks. Stream drop or command completion interrupts an
unfinished source rather than declaring success.

The application classifies literal accepted commands through `EventListener`.
Core captures only complete successful noninteractive output ranges with unchanged
epochs. It keeps the VT text, cursor and history intact, adds row fingerprints,
and declines overlapping native markers or oversized ranges. Bash history
suppression must not reuse a previous command; shell text is never re-evaluated.

Presentation-origin and snapshot-row APIs let later renderers project output
without mutating VT cursor coordinates or PTY dimensions. Existing callers retain
their ordinary viewport behavior until a presentation origin is supplied.

## Rejected alternatives

- GUI types or Markdown/Git parsing in the terminal core: reverses ownership.
- Source ownership by absolute line number: loses identity during reflow.
- Advancing only the frontend cursor for placement: disagrees with ConPTY.
- Treating captured grid text as exact stdout: discards the VT transformation.
- Capturing failed, alternate-screen or partial-line output: can cover prompts or
  reinterpret interactive applications.

## Consequences

Recognized completed commands retain a bounded textual snapshot in addition to
their grid cells. The source budget excludes frontend parser/font/GPU state.
No new presentation is enabled or persisted by this transport-only boundary.
Consumers must validate the current markers/fingerprints before covering output.

## Validation

Regression coverage checks packet validation, incremental limits, contiguous
placement, weak ownership, interruption, reflow, resets, command failures and
alternate-screen rejection. Mixed ordinary command output and OSC 7501 records
are fed at every byte split to preserve both protocols and the original grid.
The shell fixture checks ignored history and literal pipelines without executing
them again. This coverage is not a native renderer or latency acceptance claim.

## Supersedes

None.

## Revisit when

A standard protocol, persistent source storage, exact byte-stream capture, or
measured storage/throughput costs require a different boundary.
