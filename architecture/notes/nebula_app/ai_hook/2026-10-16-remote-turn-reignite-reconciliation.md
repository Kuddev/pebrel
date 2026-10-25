# Remote turn stall reconciliation becomes bidirectional

## Status

Proposed; under maintainer review. The earlier fork implementation only
settled stalled turns from idle screens; this change also lets working
screens recover a turn whose ignition event was lost. Neither direction
has been merged into main at this review point.

## Context

On 2026-10-15 a user reported: on an SSH host, the first Claude Code turn
tracks completely (UserPromptSubmit → Running, Stop → ready), but from the
second turn on the sidebar stays pinned at ready while the screen clearly
shows the turn's spinner. The hook events that ignite a turn
(UserPromptSubmit, and permission answers PostToolUse/PermissionResult
moving Blocked → Working) travel over the same lossy remote bridge as the
Stop event, so every ignition is as losable as the completion. The 1 Hz
watchdog already samples `detect` each second when `allows_screen()` is
false; only the state machine's reverse branch was missing.

## Evidence

- The remote bridge drops events when its channel lock stays busy for
  0.3 s, when the hook's own `timeout:3` fires, or when an envelope
  exceeds its size cap; these losses are observable on real hosts.
- Reconciliation previously matched only the idle prompt shape; a working
  spinner shape fell into the same no-match path that clears samples, so
  a turn that lost its ignition could never return to Working.

## Decision

Symmetric completion with zero new sampling, timers or IO:

1. `reconcile_stalled_turn` becomes bidirectional behind the shared stall
   and `pending_submit` barriers: two idle samples settle the turn to
   Done/Idle (unchanged); two working samples reignite Working and backfill
   `turn_observed` so a late Stop settles as Done rather than Idle. Each
   direction counts its own samples (`idle_samples`/`working_samples`) and
   any opposite-shape sample resets both — an alternating shape is not a
   stable form. Blocked/Done/Unknown detections reset both directions:
   the permission screen is attention semantics owned by Notification
   hooks and `observe_screen`.
2. The call site drops its `status == Working` one-way gate: a Done/Idle/
   Blocked leftover can equally be pulled back to Working (a lost
   permission answer is the mirror of a lost UserPromptSubmit);
   `next == status` short-circuits inside the state machine without a
   log or notify.
3. `apply_hook`/`input_sent` clear both counters: an authoritative event
   or user keystroke invalidates screen-reconciliation evidence.
4. Identity and completion-notification semantics are unchanged: the
   session identity and `running_program` are not cleared, and no
   completion notification fires — the late real hook arrives normally
   and is deduplicated per turn.

Review corrections (2026-10-24, maintainer counterexamples):

5. The nested-shell OSC 7 weak edge no longer clears a weakly inferred
   running command on the cwd report alone: OSC 7 is a directory signal
   and a running command can emit it (`bash -c 'printf OSC7; sleep 60'`).
   The clear now requires prompt evidence from the pane process tree —
   every process is the root session shell or a nested interactive
   shell; any command child vetoes. SSH panes keep the prompt-render
   inference (their process tree lives remotely; known boundary).
6. A reconciliation settle to Done/Idle is marked as **inferred**: the
   duplicate-completion guard only rejects replays of authoritative
   completions, so a late real TurnDone with the same turn id is
   accepted and its success/failure outcome and notification land
   (failing outcomes settle to Idle, not a ready Done).
7. The `pending_submit` barrier lives only until hook silence crosses
   the stall threshold: when a submission and every subsequent hook are
   lost, the barrier expires on the first stalled tick (samples reset),
   and the normal two-stable-frames evidence applies in both
   directions.
8. The waiting period starts at the latest explicit submission or accepted
   hook, not just the last hook from an earlier turn. Otherwise a new
   submission after a long idle period is immediately classified as stalled,
   and the old idle frame can manufacture completion within three samples.
   `turn_quiet_for` reads this shared clock; plain Enter, screen samples and
   rejected foreign hooks do not postpone recovery. Each new submission and
   accepted hook renews the entire waiting period.
9. Integration with the screen-attention expiry fix exposed two running Claude
   layouts with custom footers and no interrupt hint. A generic six-row prompt
   search misclassified both as idle. Preserve the four-row generic rule and
   recognize the captured extended footer only with its mode-switch control.
   The custom layouts remain unclassified, retaining the shared owner's stronger
   working evidence; the captured extended idle and interrupt layouts still match.

## Rejected alternatives

- Fixing only Done → Working and leaving Blocked: permission answers and
  UserPromptSubmit share the same lossy channel; fixing half the losses
  leaves the other half of the pinned states.
- Relaxing the stall threshold or igniting on one sample: a single
  spinner frame can be a redraw gap or scroll residue; two samples plus
  20 s of hook silence keep the false-positive defenses at the same
  magnitude as the convergence direction.
- Widening the remote installation surface (hook timeout, channel lock):
  rolls the manifest asset hashes for every host and does not repair
  losses that already happened.

## Consequences

- Local panes are unaffected (`reconcile_remote` gate untouched).
- Remote turn visibility no longer depends on a single hook event
  surviving: after the screen reignites Working, a late real
  UserPromptSubmit still overwrites authoritatively (both Working,
  harmless).
- Remaining false-positive surface: a Claude startup animation lasting
  > 20 s that hits the working rule twice would ignite a SessionStart-only
  session; measured startup animations vanish in seconds and the first
  real event overwrites authoritatively.

## Validation

- `ai_hook/lifecycle/tests.rs`:
  `stalled_turn_reconciliation_reignites_a_lost_prompt_submit` (bidirectional
  loop plus alternating-shape reset),
  `stalled_turn_reconciliation_reignites_a_blocked_turn_after_the_answer`,
  extended `stall_reconciliation_honors_the_pending_submit_barrier`.
- `terminal/view/activity_tests.rs`:
  `remote_lost_prompt_submit_is_reignited_by_stalled_working_screen`
  (full first turn → lost ignition → no reignite while the hook timeline
  is active → two spinner samples reignite Running with identity kept).

- Review-driven regressions:
  `a_running_command_emitting_osc7_does_not_finish_itself` (live command
  child vetoes; the settle only after it exits),
  `late_authoritative_turn_done_supersedes_an_inferred_done` and
  `late_failed_turn_done_supersedes_an_inferred_done` (late success and
  failure after an inferred Done),
  `pending_submit_barrier_expires_once_hooks_stay_silent_past_the_stall`
  and `expired_submit_barrier_lets_a_later_idle_screen_settle` (recovery
  boundary in both directions).
- `fresh_submission_restarts_the_remote_stall_clock_after_an_old_hook`
  exercises the production timeout decision with an old prior hook, a fresh
  submission, later barrier expiry, repeated submission, and accepted versus
  rejected hooks. It reproduced premature timeout before the clock fix.

## Supersedes

None. (An earlier unidirectional iteration existed only in the
contributor fork; this note records the complete mechanism.)

## Revisit when

- The remote bridge gains reliable transport (queueing/retry): both
  directions can retire together or the thresholds can tighten.
- The working-screen rule meets a new false-positive shape (e.g. an IDE
  integration screen): harden the rule before adding timers.
