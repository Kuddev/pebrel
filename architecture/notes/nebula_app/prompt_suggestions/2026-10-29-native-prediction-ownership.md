# Native next-message prediction ownership

## Status

Implemented core; native adapters and application configuration are in progress.
The core alone does not enable predictions in a CLI.

Prompt policy is superseded by the
[upstream prompt boundary](2026-10-29-upstream-prompt-boundary.md), which also
clarifies differences between this client and native upstream generation.

## Context

The requested feature predicts a user's next message in an AI CLI's empty native
composer after a completed turn. It must preserve the CLI's draft, acceptance
keys and submission semantics, while using an independently selected provider.
Shell completion and continuation of nonempty drafts are separate features.

## Evidence

- Codex's historical composer separates a candidate from draft text and scopes it
  to a completed turn, with cancellation on editor/lifecycle changes. The prompt
  predicts the user's intent and voice from conversation context. See upstream
  [prompt](https://github.com/openai/codex/blob/814de47b69dd63a2660fd14f9af66690888d183e/codex-rs/tui/assets/prompt_suggestion.txt)
  and [generation](https://github.com/openai/codex/blob/814de47b69dd63a2660fd14f9af66690888d183e/codex-rs/tui/src/app/prompt_suggestions.rs).
- That generation path uses an ephemeral fork and reloads configuration. A
  terminal application can instead reuse a prepared provider snapshot and a
  bounded native-message context without starting another Agent run.
- The local Claude Code 2.1.310 native composer API was exercised on a Linux
  desktop: a Chinese suggestion remained outside the draft until Tab, Tab did
  not submit, and typing preserved the user's own text. Extracted proprietary
  source and QA evidence are kept outside this repository.
- Claude's [memory documentation](https://code.claude.com/docs/en/memory)
  distinguishes durable user/feedback notes from conversation context. The
  presence of that system is not evidence that every prediction writes memory.

## Decision

- Native adapters own editor availability, draft revision, completion events,
  cancellation and native suggestion display. The inference core never reads
  the screen, reconstructs a draft from keys, or writes to a PTY.
- `Identity` binds a result to agent, session, completed turn, input revision,
  provider and language. A turn is attempted once; dismissal cannot trigger a
  repeated paid request. Native adapters must also reject stale results before
  asking the editor to display them.
- `PreparedClient` owns an immutable provider/credential snapshot. Prepare it
  off the UI thread when enabling or changing configuration, reuse it through
  an owning service, and cancel/replace it when settings or credentials change.
  The request path reads no provider file, keychain or transcript.
- HTTP connections are pooled, with two simultaneous inference slots, a five
  second deadline, bounded response bodies, and no retries or redirects.
  Cancellation currently suppresses publication; a blocking HTTP request may
  occupy its slot until the deadline. Do not claim immediate network abort.
- Context contains only bounded user/assistant text from native messages.
  Long messages retain both the opening request and final correction/result.
  A separate small set of user wording examples prevents long assistant text
  from becoming the inferred voice. There is no second model call for style.
- Suggestions are one bounded JSON field, or explicit abstention. Invalid,
  truncated, refused or ambiguous results never become drafts. App language
  governs natural language; identifiers remain unchanged.
- This core does not persist predictions, user wording examples or transcripts.
  Durable style memory needs its own evidence, ownership and write policy; it
  must not be introduced implicitly as a side effect of generating suggestions.

## Rejected alternatives

- Screen markers, layout heuristics and raw-key draft reconstruction: they
  cannot establish native editor ownership and create per-version maintenance.
- A second editor: it replaces the native interaction the feature must retain.
- Loading provider files, credentials or complete session logs each turn:
  repeated I/O is unnecessary when the native integration already has context.
- Persisting every prediction or rebuilding a personality profile each turn:
  no demonstrated benefit justifies the extra writes, requests and latency.
- Reusing the Agent's main model unconditionally: it prevents independent
  provider/model selection and couples prediction cost to the coding task.

## Consequences

Each CLI needs a verified native integration; an internal object name alone is
not a callable API. Unsupported versions must not silently use screen guessing.
An adapter and settings owner remain necessary before end-user enablement.

## Validation

- Local and supplied Linux product builds succeeded with existing warnings.
- Nine core/client tests passed on each host. They cover stale ownership,
  one-shot acceptance, bounded UTF-8 excerpts and voice evidence, output
  validation, actual HTTP, cancellation and redirect refusal.
- Local file-syscall tracing of the nine tests found loader/terminfo startup
  reads and no provider, credential, transcript or prediction-state file I/O.
  This is evidence for the prepared request path, not a whole-app zero-I/O claim.
- The earlier eight-case Sol evaluation used the actual Rust client on Linux;
  valid results took 1.6–4.2 seconds. It does not establish subsecond latency.
  Changed prompt wording requires a fresh semantic evaluation.

## Supersedes

None.

## Revisit when

A native API changes, immediate HTTP cancellation is required, a new provider
protocol is added, or evidence supports a bounded durable style-memory design.
