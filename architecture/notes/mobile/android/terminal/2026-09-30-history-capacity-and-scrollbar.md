# Bounded mobile history and a draggable viewport indicator

## Status

Implemented. Device acceptance is required before publication.

## Context

The native terminal retained a fixed 2000 history rows without a settings entry or
a visible position indicator. The maintainer requested a default of 1000 rows,
memory-aware limits and a scrollbar that can be dragged without selecting text.

## Evidence

The pinned C header describes `max_scrollback` as rows, but its C wrapper forwards
the number unchanged to `Screen.Options.max_scrollback`, which is explicitly bytes.
The device row-limit regression exposed this mismatch. `page.zig` defines both
Cell and Row as packed 64-bit values. The C API also exposes a scrollbar tuple of
total rows, viewport offset and visible length, but no runtime capacity setter.

## Decision

- `DisplayPreferences` owns the persisted row count, with a 1000-row default.
  Low-RAM devices or memory classes at or below 128 MiB offer up to 1000 rows;
  other devices offer up to 5000. These are product limits, not a memory-cost formula.
- Apply a changed count to newly created local/SSH terminals. Do not recreate a
  running terminal or silently discard its contents to apply a preference.
- Keep a logical history window of at most the requested rows. Independently cap
  native history pages at 16 MiB, budgeting for the supported 400-column maximum,
  Cell/Row storage and page overhead. This is a byte budget, not an assertion of
  exact per-row cost; complex graphemes may reduce available history under it.
- Fetch scrollbar metadata once per visible native snapshot, not per cell or in
  a separate polling loop. The frame's existing metadata fields keep their positions;
  the appended fields are optional for desktop frames and prepared test fixtures.
- Dragging the overlay changes viewport position through the terminal state worker.
  It does not send shell input, resize the PTY or mutate terminal text. Desktop
  mirrors use the same overlay geometry for the already available projected frame.
- Keep the narrow visual thumb separate from its expanded edge hit region. Only
  the thumb region starts dragging, leaving normal text gestures with the view.

## Rejected alternatives

- Recreate live sessions when the setting changes: destroys the user's working state.
- Interpret a row-capacity preference as a swipe-speed multiplier: different semantics.
- Inject arbitrary navigation keys to emulate history movement: may operate the
  running TUI or command editor rather than move the terminal viewport.
- Add a second history store in the view: duplicates native ownership and memory.

## Consequences

Both limits apply per terminal; native active pages and other engine structures are
additional, and this is not a guarantee about total app memory across unlimited sessions.
A desktop mirror can scroll only content supplied by its source;
the overlay does not manufacture remote history or claim that all alternate-screen
applications expose scrollback. Existing keyboard, selection and clipboard behavior
must remain independently testable.

## Validation

Preference tests cover default, clamping and persistence. Native tests cover retained
rows, absolute positions and a real Home-to-terminal scrollbar drag over 400 output
lines. Clipboard regressions remain in the same real-device interaction flow.

## Supersedes

None.

## Revisit when

The upstream API supports changing limits without destroying live contents, or
measurements justify different device classes or per-session limits.
