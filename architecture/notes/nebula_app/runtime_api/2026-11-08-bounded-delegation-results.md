# Bounded delegation results and stable callback retries

## Status

Implemented locally; pending maintainer review.

## Context

WSL completion routing worked, but callbacks still used the former product name,
JSON-escaped multiline answers and a long repeated instruction. Target pane,
generation and duration were absent. Results exceeding 4000 Unicode characters
were shortened without a machine-readable flag. A remote envelope exceeding
64 KiB discarded its entire answer while retaining lifecycle facts.

Callback changes also need to respect recurring model-input cost and avoid
duplicating results or reusing a preview from another turn.

## Evidence

The review reproduced a 4100-character result returned as 4000 characters plus
an unmarked ellipsis. Production Python send() dropped a 90000-byte answer from
the oversized remote envelope. Native wait() timed out independently while its
delegation remained pending and could complete later.

## Decision

The runtime delegation module owns correlation, bounded output formatting and
retry identity. The public endpoints and receipts remain in the API facade.
The display prefix is pebrel; existing wire protocol identifiers stay compatible.

Callbacks include the target pane/generation, explicit outcome and elapsed_ms
from registration to accepted completion, excluding later origin-busy delivery
delay. Completeness is true/false where known, or unknown for a legacy message
whose upstream bound cannot be established. Only exceptional results include
the limit, original size, reason or missing-output flag.

Normal short answers keep the single-line JSON scalar. Multiline answers retain
physical newlines through the existing guarded Agent paste path. Unsupported
paste mode falls back to the same result as one JSON-escaped line. Both paths
enforce the local-context boundary, size validation and exactly one submission.
Terminal control characters are rendered as explicit escapes, not executed.

The existing 4000-character result budget remains. Remote compaction keeps a
bounded preview in reserved bridge metadata, independently validated by the
receiver. Provider-supplied copies of this reserved metadata are removed by the
sender. Partial previews do not become complete reader answers. If the envelope
cannot fit even the preview, the missing data remains explicit.

One short, fixed untrusted-data instruction accompanies every result. There is
no extra model call for summarization or compression and no full terminal tail.
Callback content and elapsed time are generated once at completion and reused
unchanged on retry. The queue deduplicates by task id; pane/session/generation
guards remain authoritative. Stream cache files still contain sequence counters,
not answer text. No persistent answer cache is introduced.

Model configuration, system instructions and earlier conversation messages are
not rewritten for this formatting change. Provider cache-hit behavior remains
outside the runtime contract. The short-output regression bounds serialized byte
overhead against the old template; it does not claim provider-independent tokens.

## Rejected alternatives

- Replace the instruction with an unexplained tag or rely solely on a skill:
  not every receiving CLI is known to have loaded that skill.
- Unescape newlines directly into prompt(): its one-line control-character
  boundary protects terminals that cannot accept safe multiline input.
- Increase the result limit or fetch terminal history automatically: recurring
  context cost grows and unrelated output can replace the final-answer contract.
- Reformat retries using the current clock: the same result acquires different
  content and obscures duplicate delivery.
- Treat a wait timeout as task cancellation: the provider may still be running,
  and a late completion must not consume a subsequent delegation.

## Consequences

No new dependency, model call, task timer or persisted format. Legacy wire
identifiers remain accepted. A missing upstream bound is reported as unknown,
not fabricated as a complete answer. The source reader retains its existing
missing/too-large behavior for answers unavailable in full.

## Validation

Unit-test data and the legacy session/detection adapters share a process-local
private directory when no explicit test override exists; they do not mutate
process environment variables or the ordinary user workspace.

Native tests cover short-output overhead, Unicode limits, physical multiline
input and safe fallback, escaped controls, provider failures and immutable retry
deduplication. Python tests exercise supported provider result fields, oversized
envelopes, missing previews and no stale metadata on the next turn. Existing
routing, generation replacement and replay regressions remain required.

## Supersedes

None. Extends the callback presentation following the WSL completion-ownership
decision under ai_hook/2026-11-08-wsl-delegation-completion.md.

## Revisit when

The runtime gains a typed conversation-result channel, a task-deadline contract,
or providers supply a shared stable mechanism for result caching and token budgets.
