# Independent mobile history viewport

## Status

Proposed for review with the history viewport implementation.

## Context

The phone previously received only the desktop viewport. Local panning stopped at
that small grid; forwarding more wheel events moved the desktop and required input
permission even for reading. Adding inertia alone could not expose older output.

## Evidence

- `nebula_app/src/runtime_api/terminal_read.rs` captures live, desktop viewport and
  independent history from the same terminal grid, without changing its offset.
- `nebula_terminal/src/grid/mod.rs` owns the dropped-row counter used by prompt
  marks. Append and eviction preserve absolute physical coordinates; column reflow
  can change the identity of physical rows.
- `TerminalSnapshotView`
  owns local pixel movement and frame-to-frame reading anchors.
- `mobile/android/app/src/main/java/io/github/kuddev/pebrel/mobile/session/SessionRepository.kt`
  owns one reader, page revision and cancellation. Discarded replies also invalidate
  the decoding baseline, preventing an unchanged delta from reusing an older UI frame.

## Decision

Negotiate `pane.read.screen.history.v1` and request at most 200 rows / 40000 cells
per page. Return absolute range metadata with the same revision as its cells.
The phone prefetches overlapping pages near the local viewport edge; normal history
is read-only and never resizes the PTY or moves the desktop viewport.

Phone-only reflow retains source-cell offsets so replacement pages preserve the
reading anchor and fractional pixel offset. Rendering still visits visible rows.
Alternate-screen and mouse-tracking applications retain the live grid and existing
permission-checked wheel route; their screen is not normal scrollback.

History uses the existing single-flight, wakeable reader because the current screen
subscription has no page-anchor update operation. Other desktops retain their
negotiated stream/viewport behavior. No input request is replayed.
Native fling timing continues while a page is in flight and stops on new touch,
selection, zoom, input-owner change or detachment.

## Rejected alternatives

- Wheel-only history: couples reading to desktop input and cannot provide local
  continuity for a phone taller than the desktop terminal.
- Unbounded history transfer: multiplies network, allocation and layout costs.
- Desktop PTY resizing to match the phone: changes running applications for the PC.
- Separate copied terminal history: introduces another authority for retention and
  terminal control-sequence interpretation.

## Consequences

The phone can read older output with input permission disabled. A page boundary
may still expose network latency; paging does not promise offline access to rows
not yet received. Column reflow and buffer replacement do not preserve logical
line identity. A new anchor is bounded to the current retained physical range.

## Validation

Existing Android test suites cover local fling and cancellation, page replacement
with/without phone reflow, read-only history negotiation, metadata bounds and delta
revision ownership. Rust tests in `terminal_read.rs` cover append/eviction,
desktop-offset independence, cell budgets, application modes and invalid requests.
Execution results belong in the PR; automated tests do not establish device-level
latency or visual acceptance. Physical-device verification remains separate.

## Supersedes

None.

## Revisit when

The subscription protocol supports changing page anchors, or real device measurements
show repeated page-edge stalls requiring a larger bounded prefetch window.
