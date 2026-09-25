# Presentation-only cursor motion

## Status
Accepted for local implementation, 2026-09-25. Native validation is recorded separately.

## Context
The requested opt-in should reproduce silkmux's 90 ms cursor movement without changing terminal input, parsing, or process ownership.

## Evidence
`terminal/element.rs` paints directly from `RenderSnapshot`. GPUI provides frame requests and a reduce-motion preference. The pinned vte 0.15 `Processor` buffers synchronized-update bytes until ESU/timeout; `event_loop::tests::animation_snapshots_cannot_observe_a_partial_synchronized_update` exercises the production StreamProcessor and render snapshot across chunk boundaries.

The reference is silkmux `CursorMotionTracker` and its projection retreat guard. Flutter's named easeOutCubic is the cubic Bezier (0.215, 0.61, 0.355, 1), with x tolerance 0.001, not the commonly substituted polynomial.

## Decision
Each TerminalView owns a motion tracker. It accepts monotonic time, uses cell coordinates and starts a new 90 ms motion from the current visual position. Distances above eight cells snap. Settings/visibility, screen, viewport and metric changes invalidate motion. Blink and terminal hiding remain distinct.

Only the visual target is guarded against transient prompt retreats; the authoritative grid and input/IME coordinates remain current. Explicit navigation gets bounded, expiring input authorization. Alternate-screen application positioning bypasses prompt heuristics.

Use frame demand only while moving or confirming a bounded candidate. Keep existing snapshots and the single parser; no new terminal-core state or production dependency. Solid block inversion is clipped to the moving rectangle while glyph geometry remains fixed.

## Rejected alternatives
Boolean UI controls violate the requested dropdown. A fixed timer adds idle work and an unrelated lifetime. Moving logical coordinates would corrupt selection/IME. A second snapshot authority is unnecessary for the pinned parser's buffered synchronized updates. Moving application-drawn cursor glyphs would alter program content.

## Consequences
New animation frames still run the existing terminal renderer. Measure actual cost and stop frame demand when hidden, disabled or settled. Application-owned fake cursors retain their current rendering. Preserve source attribution if reference code is copied; this implementation expresses the behavior independently in Rust.

## Validation
Deterministic trajectory, input, discontinuity and lifecycle tests; parser snapshot regression; settings round-trip and real Select interaction; native visual and cost checks. Test completion is reported in local research receipts, not inferred here.

## Supersedes
None.

## Revisit when
The VT parser changes synchronization behavior, a measured multi-pane regression requires caching, or explicit support for application-owned cursors is requested.
