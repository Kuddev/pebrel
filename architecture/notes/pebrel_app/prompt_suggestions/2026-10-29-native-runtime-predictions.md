# Native editor adapters and one resident prediction service

## Status

Implemented integration for verified Claude Code, Pi and OpenCode APIs, with
Linux desktop product acceptance completed. Codex and Qoder do not yet have an adapter
that satisfies independent-model access to their running native TUI editor.

## Context

The requested first increment predicts one next message after a completed turn,
only while the native input is empty. Users select a common provider/model and
software language. Continuation and durable voice profiles are out of scope.
The user's latest direction makes current conversation context the voice source.

## Evidence

- The [pinned Codex prompt boundary](2026-10-29-upstream-prompt-boundary.md)
  establishes exact prompt bytes and the user-fragment role. Its native fork
  inherits complete effective Agent context and the original provider; an
  independent provider with bounded text is a deliberate product difference.
- Claude Code 2.1.310 provides native `prompt.read`, `prompt.suggest`,
  `session.messages`, `turn.complete`, prompt lifecycle hooks and `PromptHint`.
  The integration requires its function-hook flag and a process-local plugin
  directory. `session.end` clears state; `/clear` does not necessarily repeat
  `session.start`. A zero-delay native timer leaves the completed-turn hook
  before performing optional prediction work. Proprietary source is not vendored.
- Pi 1.4.0 exposes `CustomEditor` through its extension SDK. OpenCode 1.19.5
  exposes the `session_prompt` slot, `ui.Prompt` and `TuiPromptRef`. The OpenCode
  callback receives `(context, props)` and the plugin has a default `{id,tui}`
  export. A keyed session component disposes the previous controller.
- Existing application runtime discovery is per instance and already binds a
  loopback listener. Adding another daemon or a PTY proxy would duplicate
  lifecycle and cannot itself expose the editor's semantic draft state.
- Native settings acceptance found that the existing provider route was hidden
  from navigation and search. Placing the new prediction controls there without
  restoring the route left both provider setup and prediction settings unreachable.

## Decision

- Native adapters alone own input availability, session/turn/revision identity,
  cancellation and acceptance. They pass bounded in-memory user/assistant text.
  The host never reads transcript files, screens or raw keys to rebuild a draft.
  The unused host-side editor-state prototype is removed; lifecycle regressions
  exercise the native adapters and the resident service that actually own state.
- The host preserves actual conversation roles and appends the unchanged Codex
  prompt/schema inside the upstream user-fragment marker. The only additional
  instruction requests the configured software language in English. There is
  no separate `user_voice_examples` field, style classifier or persisted profile.
- Context remains bounded to eight messages and 12,000 characters, with long
  messages retaining both ends and budget reserved for the most recent user
  message. Multiple long assistant messages must not evict that user request. This is not represented as the full native
  Codex fork or as evidence of identical model quality.
- The resident runtime owns one prediction service. Native HTTP clients share
  its loopback listener but receive a separate random prediction-only token.
  That token cannot authenticate JSONL runtime control. Only config/prediction
  HTTP routes exist, without CORS, redirects or key retrieval.
- IPC bodies are limited to 128 KiB, headers to 8 KiB, and model responses to
  64 KiB. The model client has two concurrent slots, a five-second deadline and
  no automatic retries. Native adapters reject delayed results on edits,
  activity, session changes or disposal; a completed turn is attempted once.
- Settings/provider/key writes invalidate a shared revision. A background IPC
  worker refreshes the immutable provider/key snapshot only after such changes;
  UI rendering never reads credentials. Model calls release the service lock.
  Configuration changes suppress pending results. No transcript or prediction
  state is persisted. Changes apply to subsequent completed turns.
- The existing provider route is visible again, with prediction search aliases.
  Its stable route ID and shared provider/credential store remain unchanged.
  Prediction save writes only its enabled/provider/model fields; it does not
  invoke provider metadata save or apply a provider to any CLI's main model.
- Cancellation suppresses host publication; the blocking model call can retain
  its slot until timeout. Do not describe this as immediate provider-network abort.
- Claude uses its native ghost suggestion/Tab acceptance. Pi and OpenCode use a
  gray, read-only hint inside their official editor components, preserving the
  existing input, IME, attachments and keymap. They do not infer input bounds or
  implement another textarea. Loading uses the localized model name with a
  400 ms `.`, `..`, `...` cycle and is never inserted or accepted.
- The shared JavaScript module owns transport and transient lifecycle only;
  prompt/model policy stays in the host. Source-version compatibility is checked
  before launch. Unsupported versions keep the original CLI behavior.
- A thin process-local launcher installs a content-versioned bundle in Pebrel's
  own runtime directory, then runs the original executable on the same TTY.
  It preserves user-edited files and CLI global settings. Local GUI start/resume
  commands can use it; SSH/WSL commands never receive local filesystem paths.
  Manually typed native CLI commands need the explicit prediction launcher.

## Rejected alternatives

- A second editor, raw-key reconstruction, prompt-character matching, or parsing
  cursor markers from rendered lines: none establishes native draft ownership.
- Editing closed-source bundles or maintaining a Codex fork just to expose
  private editor state: these undermine the requested low-maintenance boundary.
- Calling a headless model policy or an unimplemented public schema a working
  TUI integration. Codex/Qoder remain unsupported until a verified route exists.
- Changing a CLI's main provider or global prediction settings: the selected
  prediction model must be independent and scoped to this launched session.
- Reading credentials/history or writing voice memory each turn: native context
  and an immutable resident snapshot make those costs unnecessary.

Native executable resolution and terminal-preserving process handoff live in
`platform/agent_launch.rs`; the prediction launcher owns only plugin arguments,
compatibility and localized reporting. Filesystem fixtures use the platform alias
helper so source-level platform branches do not spread into the adapter.

## Consequences

Native acceptance stays explicit and never submits automatically. The settings
feature is off by default. Capability and acceptance evidence are specific to
verified CLI versions; protocol names and compatibility names remain in product
text where necessary. API calls may still take seconds; a loading indicator is
feedback, not proof of subsecond model latency.

## Validation

Core regressions cover exact prompt bytes, real HTTP parsing, output boundaries,
configuration invalidation, repeated turns, authentication, malformed framing
and unsupported routes. Settings cover defaults, validation, round trips and
reset. Native adapters have independent lifecycle tests and real native SDK/TUI
probes; those do not replace the integrated product/provider acceptance.

The Linux x86_64 GPUI product at `a0801cb4` built successfully. Claude Code,
Pi and OpenCode each passed two turns through the actual launcher, installer,
resident service and native editor using an offline model fixture. Desktop
checks covered all three loading-dot states, accepting without submission and
rejecting late predictions after typing. Claude clears its loading hint before
the native ghost suggestion appears. Pi/OpenCode render native editor hints,
not inline cursor ghost text. These checks establish integration, not model quality.

Actual settings controls passed provider selection, independent model saving,
enable/disable, invalid-draft feedback, reopening and application restart.
Prediction-only saves left provider metadata byte-for-byte unchanged, including
the CLI main model and active-provider mapping. The panel remained readable at
850 px window width. The initial desktop lacked Secret Service components and
the existing provider editor reported the dependency error. A follow-up run
used an isolated HOME, D-Bus session and actual Secret Service daemon: creating
a provider and saving a synthetic credential through the product UI succeeded,
and an independent keychain lookup returned the same test value. The new provider
was selectable for prediction without changing its main model or metadata.
No real credentials or model requests were used in this settings check; the
temporary keychain, configuration, extracted dependencies and processes were removed.

At the same revision, prediction tests passed (21, with one paid evaluation
ignored), Runtime tests passed (97), Agent identity tests passed (47), settings
UI tests passed (27) and settings crate tests passed (107). Architecture checks
against `cf6bafb2` and the workspace formatting check passed. Linux execution
does not establish native Windows or macOS behavior.

Eight synthetic multi-turn requests using the unmodified upstream prompt and
`gpt-6.1-sol` succeeded on the supplied Linux host. Observed latency was
1.887–3.072 seconds, median 2.412 seconds; this is one run, not a performance
guarantee. The under-specified arithmetic case still produced an arbitrary
follow-up, so valid JSON and request success do not establish prediction quality.
No local prompt or voice-policy patch masks that upstream behavior.

## Supersedes

The separate transient voice-evidence field described in
[initial ownership](2026-10-29-native-prediction-ownership.md). The
[verbatim upstream prompt boundary](2026-10-29-upstream-prompt-boundary.md)
remains authoritative. No durable voice experiment is enabled.

## Revisit when

A CLI changes its native editor API, Codex/Qoder expose a verified TUI adapter,
provider-network cancellation is required, or measured acceptance justifies a
change to the bounded context/latency tradeoff.
