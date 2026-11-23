# Preserve the Codex prompt and separate verified upstream behavior

## Status

The prediction prompt and schema suffix now preserve the pinned Codex source
verbatim. The existing short-context client remains unfinished application
integration; this change does not turn it into Codex's native fork pipeline.

## Context

The requested behavior uses Codex's prediction style and learns persistence from
Claude Code. Adding local voice rules after synthetic evaluations would make
the implementation diverge from that request. Source inspection must distinguish
prompt text, model context, lifecycle control and general memory maintenance.

## Evidence

- Codex revision `814de47b69dd63a2660fd14f9af66690888d183e` declares its
  [prompt](https://github.com/openai/codex/blob/814de47b69dd63a2660fd14f9af66690888d183e/codex-rs/tui/src/prompt_suggestions.rs#L9)
  as an unchanged desktop fallback. The asset has 1,339 bytes and SHA-256
  `3728870393e8d9776f983db00b1b4bea094b541d56478807d204f7e650232f4e`.
- Its [generation path](https://github.com/openai/codex/blob/814de47b69dd63a2660fd14f9af66690888d183e/codex-rs/tui/src/app/prompt_suggestions.rs)
  resumes settings and forks through the completed turn. The fork is ephemeral,
  retains parent model/provider/settings and tools, and adds the original prompt
  plus a textual JSON schema as an `external_prompt_suggestion` user fragment.
  Its deadline is 30 seconds. These are not the same as this client's five-second
  independent provider request or eight-message text excerpt.
- Tests in that upstream path verify preservation of the parent request prefix.
  Neither a separate `user_voice_examples` field nor a dedicated style-extraction
  request is established by that source.
- Claude Code 2.1.310's installed program reuses prepared system/user/system
  context and messages for a prediction fork. It uses its own shorter prediction
  prompt and handles eligibility, throttling and cancellation in code.
- In that build, `skipTranscript` suppresses recording the prediction fork;
  `skipCacheWrite` controls model prompt-cache boundaries. The latter is not
  evidence of avoiding a local cache-file write. Proprietary source is not
  copied into this repository.
- Claude's [memory documentation](https://code.claude.com/docs/en/memory)
  describes general user/feedback/project/reference notes, an initial bounded
  index and on-demand topic reads. Inspection did not establish a separate
  persistent prediction-voice database.

## Decision

- Keep the Codex asset byte-for-byte and the schema suffix identical to its
  generation call. A digest regression protects this boundary. Preserve upstream
  attribution in `THIRD-PARTY-NOTICES`.
- Do not add local style, persona or abstention instructions in response to a
  small synthetic evaluation. First reproduce the context and lifecycle facts
  of the relevant upstream version.
- Keep evaluations labeled by the prompt and transport actually exercised.
  Earlier multi-agent voice tests exercised a product-derived prompt, not the
  unmodified upstream pipeline. Their results cannot establish upstream quality.
- Defer the experimental six-topic voice profile, evidence thresholds,
  maintenance cadence and extraction prompt. They were local designs, not
  verified Codex/Claude prediction mechanisms; they are outside the product tree.
- General memory reuse and native prediction/editor integration require their
  own verified adapters. This change neither enables all CLIs nor establishes
  cross-session persistence.

## Rejected alternatives

- Calling a derived prompt 'the Codex prompt': hides meaningful behavioral
  differences and invalidates comparisons.
- Treating a valid JSON result as semantic success, or a network failure as
  abstention: neither establishes useful intent or style prediction.
- Treating an assistant's summary as missing delivery without reading the
  fixture: one apparent counterexample already contained the requested summary.
- Equating generic Claude memory with a dedicated voice learner, or translating
  `skipCacheWrite` into a local filesystem performance guarantee.

## Consequences

The prompt now has an auditable source boundary. The client still differs from
native Codex in context, roles, model routing and timeout; those differences
remain visible until a verified native integration resolves them. No new style
memory file or maintenance service is enabled by this change.

## Validation

The prompt was compared against a fresh fetch of the exact upstream revision.
The actual GPUI product test build succeeded locally and on the supplied Linux
host. Ten related tests passed on each host; the paid live evaluation was left
ignored. Formatting and the architecture check against the feature base passed.
Prior derived-prompt evaluations are retained separately and are not relabeled
as upstream tests. Live evaluation of the complete native pipeline remains
outstanding.

## Supersedes

The locally elaborated prediction prompt from
[initial ownership](2026-10-29-native-prediction-ownership.md). Editor ownership
and independent-provider requirements remain unchanged.

## Revisit when

A newer upstream prediction implementation is selected, a verified adapter can
reuse native prepared context, or a demonstrated product requirement needs an
explicitly documented deviation from upstream behavior.
