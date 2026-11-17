# Application mouse gesture lifetime

## Status

Implemented for left-button application gestures in the GPUI terminal.

## Context

Fullscreen input editors own their draft, caret, selection, and editing commands.
The terminal owns event delivery. Previously each move/release independently chose
between application reporting and terminal selection using the current modifiers
and hitbox. A drag leaving its pane lost its release, entering another pane could
emit an unpaired drag/release, and pressing Shift mid-gesture changed ownership.

## Evidence

- The GPUI pointer regressions reproduce missing releases and foreign input using
  actual element hit testing and the bytes sent to a terminal session.
- Applications can disable and re-enable mouse reporting in one PTY batch, so
  sampling the final mode flags cannot distinguish a new input lifetime.
- Native Claude Code 2.1.310 reasserts `1000h`, `1002h`, `1003h`, `1006h` while
  interacting. Treating each tracking-detail change as a new lifetime dropped
  valid releases. The regression now covers this sequence explicitly.
- Native Codex 0.165.0 receives selection deletion through the ordinary PTY path;
  no terminal-side text deletion or draft mirror is required.

## Decision

The view records an application gesture only after encoding its left press. It
retains a weak terminal identity, the terminal's mouse-input generation, and the
last cell/modifiers. Window capture routes motion/release to this owner outside
its hitbox, before another pane handles them. Listeners exist before the press so
events between paint frames are also captured. Native selection, links and
scrollbars retain their own existing state; a foreign left drag cannot start an
application gesture.

The terminal core owns negotiation lifetime. Crossing the enabled/disabled
boundary, changing encoding, switching screens, reset, and grid resize invalidate
mouse coordinates. Reasserting tracking detail while reporting remains enabled
does not. This is a protocol/coordinate generation, not a process identity or an
application-name heuristic.

Loss of focus/visibility and layout changes release a live gesture at its last
valid cell. Late input after protocol/session replacement is discarded. A move
with no held left button recovers a missed OS release. Core generation checks
also cover transitions that occurred entirely between GUI events.

Keyboard, IME and paste keep their existing encoding/broadcast contracts. Mouse
reports target only the originating session. The terminal does not infer or copy
an application's selected text into a shell editing command.

## Rejected alternatives

- Re-evaluating Shift on every event loses the release owed to the press owner.
- Broadcasting coordinates or handing a drag to the hovered pane creates input
  without a corresponding press in the receiving application.
- Tracking only final mode bits misses disable/re-enable in one parser batch.
- Invalidating on every mode setter breaks applications that reassert modes on
  redraw; only input-lifetime/encoding/coordinate boundaries invalidate a press.
- Synthesizing Backspaces from terminal selection length cannot preserve the
  application's Unicode boundaries, placeholders, undo state, or actual draft.

## Consequences

Each view holds at most one application left gesture and registers two small
capture listeners per paint. Idle listeners only check for an owner. No gesture
timer, editor buffer, worker, persistence, or dependency is introduced.

The generation cannot identify a process replacement that does not change any
terminal protocol or screen state. Applications still must reset their input
modes on exit; the PTY does not expose the identity of a byte-stream consumer.
Right/middle-button behavior and ordinary-shell editing are outside this change.

## Validation

- `term/mouse.rs` tests negotiation in every parser chunk size, idempotent
  reassertion, encoding transitions, screen switches, reset and resize.
- `pointer/tests/application_mouse.rs` tests pane boundaries in both directions,
  Shift changes, focus/visibility, session replacement, late release, resize,
  legacy encoding, and redraw reassertion with real GPUI event dispatch.
- `application_mouse/native_tests.rs` provides an opt-in native-window/PTY fixture
  for isolated application drafts, using real keyboard and pointer dispatch.
- Linux native checks do not establish Windows ConPTY or macOS compatibility;
  those hosts require their native execution paths to be run separately.

## Supersedes

None.

## Revisit when

GPUI exposes a persistent pointer-capture owner, the terminal gains another mouse
encoding, or application mouse lifetime is exposed by a stronger protocol. Reuse
the terminal generation rather than adding per-frontend protocol state machines.
