# Idle render frame budgets for terminals and agent spinners

## Status

Implemented on 2026-10-12 (ported from the Fleet GPUI-shell fork with its
regression lessons). A follow-up snapshot cache remains a design gate only
and is intentionally not part of this change.

## Context

Idle terminals and agent "Running" badges kept whole windows redrawing at
display refresh rate: the spinner phase advanced once per rendered frame,
the cursor blink chain ticked forever even for unattended panes, every PTY
read batch issued a frame, and the 1 Hz watchdog ran a full-machine `ps`
enumeration on the UI thread (up to ~1 s stall per probe on cache misses,
measured in Fleet on 2026-10-12). The result was a constant, visible CPU
floor whenever an agent session sat waiting.

## Evidence

- Fleet CPU sampling on 2026-10-12 identified vsync-rate whole-window
  redraws driven by the spinner as the single largest idle-CPU item;
  decoupling it from the frame source dropped redraws to 12.5 fps while
  the animation remained visually continuous (0.8 s per revolution needs
  only ~10 steps).
- The watchdog stall was reproducible as periodic input latency on
  machines with large process tables.
- Fleet's first attempt at caching agent screen detection short-circuited
  the whole `refresh_agent_screen_state` section and pinned agents at
  Running forever; observe_screen's per-second sample accumulation
  (Working→Idle needs two consecutive samples) must keep its cadence.

## Decision

Each idle cost gets a bounded budget at its own source; no global frame
throttling:

1. Spinner: one 80 ms timer chain (`SPINNER_TICK`) per visible running
   badge, phase advanced by real elapsed time (decoupled from display
   refresh); a latch guarantees at most one chain, and an epoch tears down
   in-flight chains when the badge disappears or the window hides.
2. Cursor blink: after 8 s of total silence (no input, PTY output, IME or
   mode change) the blink chain stops toggling and stays solid — a
   motionless terminal produces zero render frames. Any input restarts
   the blink immediately; PTY output only refreshes the silence clock
   and restarts the chain if (and only if) it was paused, so output
   storms neither keep the cursor lit nor add restart churn.
3. Output-driven frames: PTY read batches within 8 ms of the previous
   frame coalesce into a single trailing frame (≤ 125 fps ceiling, tail
   latency bounded by the remaining interval); hiding a pane clears the
   merge baseline so the first visible batch renders immediately.
4. Watchdog probes: the `ps` enumeration runs on a background thread via
   `cached_activity_evidence` (250 ms shared snapshot); the UI thread
   only applies results, and stale batches are dropped by command
   identity.
5. Agent identify/detect memoization: single-slot memo keyed by manifest
   generation, program text and screen text — cache misses alone pay the
   gate-regex cost, never the sampling cadence (see the Fleet lesson
   above).

## Rejected alternatives

- Throttling the whole render path or short-circuiting
  `refresh_agent_screen_state`: breaks per-second sample accumulation and
  pins agent state (Fleet regression).
- Caching render snapshots in this change: needs a render generation
  counter in `TermDamageState` plus race guards; kept as a separate
  design gate instead of half-shipping it.
- Pausing the spinner when the window is inactive: the badge is exactly
  how a user notices a background agent is still working; the 12.5 Hz
  chain already makes the idle cost negligible.

## Consequences

- Idle CPU for parked terminals and waiting agent turns drops to near
  zero without touching animation semantics or detection cadence.
- Each budget is local and independently testable (pure decision
  functions: `cursor_blink_silence_pauses`, `output_frame_timing`,
  `advance_spinner_phase`).
- A future snapshot cache must bump a generation on every printable
  write and re-read the generation after capture; this change keeps that
  door open but does not walk through it.

## Validation

- Unit tests for the three pure decision functions
  (`terminal/view/tests.rs`, `display/animations.rs` spinner phase test).
- Full `cargo test -p nebula` green; manual Fleet-derived measurements
  documented above.

## Supersedes

None.

## Revisit when

- Implementing the render snapshot cache: start from the generation
  design gate recorded alongside the Fleet lessons.
- A display sync protocol makes 8 ms/80 ms budgets tunable per monitor.
