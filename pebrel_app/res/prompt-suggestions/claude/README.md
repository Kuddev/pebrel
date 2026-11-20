# Claude Code native prompt predictions

This managed adapter uses Claude Code's native function hooks. It is verified
against Claude Code 2.1.310; older versions are not claimed as supported. It is
loaded for a new process with `--plugin-dir <absolute-adapter-directory>` and
`CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1`. Loading an adapter does not install it into
the user's global Claude configuration.

The launcher must provide `PEBREL_PREDICTION_ENDPOINT` (an HTTP IPv4 loopback
address with a port), `PEBREL_PREDICTION_TOKEN`, and
`CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false`. The last variable suppresses Claude
Code's built-in prediction inference for that process. Without it the adapter
does nothing, avoiding two inference requests for the same completed turn. No
global setting or main-model setting is changed.

Only a successful main turn with an empty, mounted terminal composer is eligible.
`$.session.messages()` supplies in-memory user/assistant text; tool payloads are
not sent. The bridge receives at most 24 messages, 3000 UTF-16 units per message,
and 12000 units total. Pebrel owns the prediction prompt, configured provider,
language, and further context limits. The adapter adds no speaking-style prompt
or persistence.

The localized bridge `loading_text` appears in Claude Code's dim `PromptHint`
footer, followed by `.`, `..`, and `...` every 400 ms. It is noneditable and is
never sent to `prompt.suggest`. A completed candidate uses native
`$.prompt.suggest({text})`, so native Tab/right-arrow behavior fills the composer
without submitting it. The composer remains owned by Claude Code.

Editing, prompt changes, new turns, session end, and a ten-second deadline
invalidate pending UI work. Every result rechecks the revision, session, and
empty draft. The native `http.fetch` API has no abort signal: cancellation here
suppresses stale results, while the bridge owns the network/request deadline.
No disk polling or timers remain when prediction UI is idle.

Run the behavior contract with `node --test tests/*.test.mjs`, and the actual
Claude scanner with `claude plugin validate --strict --json <adapter-directory>`.
Mock tests do not establish native terminal visual acceptance or compatibility
with an untested Claude version.
