# Keep color-report subscriptions silent

## Status

Reviewed for integration on 2026-09-26.

## Context

Issue [#283](https://github.com/Kuddev/pebrel/issues/283) reports literal
`^[[?997;1n` input around Fish prompts in native SSH panes, sometimes twice per
command. The terminal emitted this report every time DECSET 2031 was enabled,
even when the palette had not changed.

## Evidence

- [Fish's compatibility contract](https://github.com/fish-shell/fish-shell/blob/966dbbbd4de0b0efcf27275e465d081de900d9a8/doc_src/terminal-compatibility.rst#L226-L233)
  defines `CSI ? 2031 h` as enabling future color-theme reports and expects
  `CSI ? 997;1n` / `CSI ? 997;2n` when the theme changes. Enabling a
  subscription is not itself a change or a `CSI ? 996 n` query.
- [Fish's terminal handoff implementation](https://github.com/fish-shell/fish-shell/blob/966dbbbd4de0b0efcf27275e465d081de900d9a8/src/tty_handoff.rs#L194-L195)
  enables and disables this mode around terminal ownership changes.
- A production-core reproducer with two enables and an unchanged palette emitted
  two `ESC[?997;1n` replies. The regression added to `term/tests.rs` also failed on
  the original implementation before the fix.

## Decision

DECSET 2031 updates subscription state without emitting a report. The existing
palette-change path retains its change detection and only notifies subscribers.
Both first-time and repeated enables stay silent, including fragmented input.

## Rejected alternatives

The original immediate reply tried to compensate for vte 0.15 not routing the
private DSR query to `Handler::device_status`. Unsolicited input is not a valid
substitute for a query response: it may reach the shell during terminal handoff.
Filtering these bytes only in SSH would leave the shared terminal behavior
incorrect for other transports. Changing the parser dependency is unnecessary
for this notification regression.

## Consequences

The subscription path no longer formats or queues a response. No per-byte parser
work, timer, thread or stored state is added. Actual dark/light changes continue
to notify subscribed programs; disabled notifications and same-color updates
remain silent. The existing private DSR query limitation is unchanged.

## Validation

The regression covers both starting color schemes and every two-chunk split of
repeated enable/disable sequences. It also verifies a real color flip still
reports, repeated application stays silent, and disabling suppresses reports.
Existing direct-handler and parser tests retain their mode-state and real-change
assertions with the corrected subscription expectation.

Local Windows validation on the updated integration base passed: 277 unit tests,
45 replay cases, the connection
transport test, the bundled ConPTY lifecycle test and one doctest. The native
fixtures used the repository-pinned console runtime and an explicit Git Bash.
The original Windows-to-Debian Fish SSH session has not been replayed locally.

## Supersedes

The immediate-report rationale in `Term::set_private_mode` and its regression
expectations; no previous decision note exists.

## Revisit when

Revisit query support separately when the VT parser exposes private DSR handling.
Keep query replies separate from change subscriptions.
