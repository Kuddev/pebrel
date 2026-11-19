# Consume an ambiguously delivered GUI handover

## Status

Implemented; validation is recorded below.

## Context

Issue [#797](https://github.com/Kuddev/pebrel/issues/797) reports an Explorer
"Open in Pebrel" launch creating about 42 tabs and then another process. The
startup fallback retries up to 40 times. Previously each attempt sent `tab.new`
or `window.create`; a missing reply was treated as if no mutation had happened.
The initial attempt and final local window account for the additional launches.

## Evidence

- `runtime_api/server.rs` previously mapped every request error to `false`.
- `gpui_shell/mod.rs` retries that `false` result 40 times, then opens locally.
- A successful `ATTACH` establishes only focus/reveal, not creation completion.
- A timeout, peer close or response decoding failure after sending JSONL cannot
  prove that the resident did not already create the tab/window.
- The report does not include a transport trace explaining its original missing
  acknowledgment. The fix addresses the reproducible amplification mechanism.

## Decision

The startup handover pins the `ATTACH` and creation request to one endpoint.
Discovery or connection failure before writing remains retryable. After writing
may have begun, the launch is consumed whether the reply confirms completion or
leaves it unknown. Unknown outcomes never repeat creation or open a fallback.
A bounded, identity-checked JSONL reply is required to report confirmation.
The framing does not require a TCP write-half shutdown.

The existing boolean startup API now means "launch consumed", not "creation
confirmed". An unconfirmed result logs a localized diagnostic and writes stderr;
Windows additionally uses the existing startup error dialog. Other platforms'
startup reporter has no GUI implementation, so this change does not claim a
cross-platform native error dialog.

## Rejected alternatives

- Retrying every failure: creation is not idempotent and can execute after a timeout.
- Increasing the timeout: delays failure without establishing delivery semantics.
- Returning success silently: conceals an outcome the client cannot confirm.
- Adding deduplication only to the new server: an old resident would still receive
  repeated mutations from a new launcher during an upgrade.

## Consequences

A lost confirmation may require a manual retry after checking the resident.
The existing foreground process, saved sessions and other runtime commands keep
their ownership. No persistent request log or new background worker is added.

## Validation

Real TCP regressions cover complete, lost, delayed, truncated, mismatched and
rejected replies, plus connection refusal and the 40-attempt startup retry
contract. The positive path preserves the requested working directory.

The supplied Linux host built the actual GPUI product test binary and passed all
99 Runtime tests, including the three new regressions. Its translation contract
also passed. Local architecture validation, all 55 checker regression tests,
formatting and whitespace checks passed. Windows Explorer reproduction itself
was not run locally; platform CI is required before merge.

## Supersedes

None.

## Revisit when

A versioned end-to-end idempotency contract exists for GUI creation, or native
startup error presentation is extended to other platforms.
