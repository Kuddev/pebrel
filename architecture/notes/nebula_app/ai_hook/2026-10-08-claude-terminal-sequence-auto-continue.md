# Claude bounded automatic continuation

## Status
Proposed in PR #542; this note covers only the automatic continuation feature.

## Context
Claude can end a turn with a structured recoverable error. Repeating the same
failure indefinitely is unsafe, while requiring every transient failure to be
handled manually interrupts an interactive session. The retry decision belongs
to the accepted hook lifecycle; the pane owns the delay and input delivery.

## Evidence
- Claude `prompt_id` identifies a prompt turn and can reject late events from a
  prior turn without confusing a new retry with the previous failure.
- Keep every observed prompt ID for the current Claude session. Evicting an old
  ID can let a delayed submit look new and its failure trigger another retry;
  clear the set at an identity-bearing, non-compacted session change or when
  the owning command lifecycle resets.
- Remote OSC hooks enter the GPUI mailbox as raw envelopes and are parsed later
  by the session pump. Synchronous provider hooks do not wait for that pump, so
  submit-time lifecycle state alone cannot identify an older queued event.
- A process-wide ingress sequence can order local pipe reads and remote mailbox
  admission before either route reaches the pane lifecycle.
- The pane already owns the session, negotiated Enter sequence, input epoch,
  and PTY lifetime needed to decide whether a scheduled continuation is valid.
- A plain keyboard Enter submits Claude input through `commit_line`; Shift+Enter
  follows a separate multiline path and must not arm a turn.

## Decision
- Persist a default-on `ai_auto_continue` setting, expose it in the terminal
  alert settings, and include it in reset behavior.
- Retry only Claude `server_error`, `overloaded`, `rate_limit` and `unknown`
  failures. Limit a consecutive failure chain to three attempts, with at least
  five seconds between later attempts; a successful turn resets the chain.
- Consume each accepted failed turn once. Use `prompt_id` as Claude's turn
  identity, retain all IDs for the current session, and preserve the ID with the
  error when a large hook payload is trimmed. Reset this history at a new
  identified session or owner lifecycle boundary.
- Assign local pipe sequence numbers during parsing and remote OSC sequence
  numbers in the application event sender before queueing raw bytes. Carry the
  number through deferred parsing and reject Claude events older than the pane's
  latest input fence. Capture that fence before keyboard, runtime and automatic
  continuation input is sent; keep it across Hook processing so an old event
  cannot clear it. Only Idle/Done/Unknown plain Enter arms a new-turn guard;
  Enter answering a Blocked Claude prompt remains a same-turn input. Shift+Enter
  does not advance the fence.
- Keep the cancellable timer and negotiated `continue` input in the owning pane.
  Recheck the session, idle failed state, setting, input epoch and live PTY at
  dispatch. Never broadcast the input.
- Cancel pending work on accepted user input, composition, paste or path intake,
  setting disablement, session end and pane closure.

## Rejected alternatives
- Unlimited retries or retrying authentication, billing and unrecognized errors.
- Screen text or silence as authority to retry without an accepted hook event.
- Sending a retry through a shared/global input path that could target another pane.

## Consequences
The feature can send input automatically while enabled, so users can disable it
in settings. Retry remains independent of toast preferences and does not suppress
the failure notification. Missing session or turn identity prevents retry. The
prompt-ID set grows with the current session and resets at a new identified
session or owner lifecycle boundary; bounding it would reopen a delayed-hook
retry path.

## Validation
Validated against `upstream/main@f92b2bbd` on 2026-10-25:

- Earlier feature validation: `nebula-settings` 105 passed; i18n contract 24
  passed, 1 ignored; Hook Python 15 passed; CI contract 18 passed.
- Revalidated after the ingress fence change: `cargo test --locked -p nebula
  --bin pebrel --features gpui-test-support auto_continue` 23 passed, including
  queued remote Hook, keyboard, Shift+Enter and runtime regressions;
  `ai_hook::lifecycle::tests::` 30, `runtime_submission_codex` 5 and mailbox 4
  also passed from the same Windows test binary.
- Revalidated after retaining all prompt IDs and extending the Working-Enter
  failure case: the same four targeted groups passed (23, 30, 5 and 4 tests).
- `scripts/check_architecture.py --base f92b2bbd` and `git diff --check`: passed. The checker reported existing review-only notices for files above the soft line target.
- Targeted nightly rustfmt with Windows line endings passed for the changed GPUI regression test and session pump. The broad file check reports existing formatting differences in untouched ranges of `protocol.rs`, `event_loop.rs` and `event_mailbox.rs`; those files were not wholesale reformatted.

## Supersedes
None.

## Revisit when
Claude changes its retryable error values or `prompt_id` contract, or the retry
limit and default require a reviewed product decision.
