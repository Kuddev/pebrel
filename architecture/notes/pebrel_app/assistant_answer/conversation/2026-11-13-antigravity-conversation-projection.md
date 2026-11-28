# Antigravity conversation view transcript projection

## Status

Accepted

## Context

Mobile Conversation View (`conversation.read` / `send` / `choose` / `key`) projects a pane's live foreground CLI conversation without spawning a parallel Agent process. Previously, both `runtime_api::conversation` and Android's `"conversation"` route accepted only `codex` and `claude`, whose hook payloads report an explicit `transcript_path`.

Antigravity CLI (`agy`) reports `conversationId` through its lifecycle hooks (`SessionStart`, `PreInvocation`, `Stop`), but its hook payload does not carry a `transcript_path`. Users opening Conversation View on an active Antigravity pane saw the placeholder message stating that only Codex or Claude Code transcripts were supported.

## Evidence

- Antigravity writes conversation logs deterministically under `<HOME>/.gemini/antigravity-cli/brain/<conversationId>/.system_generated/logs/`, maintaining `transcript_full.jsonl` (untruncated) alongside `transcript.jsonl` (compact).
- Primary conversations begin at `step_index: 0` with `source: "USER_EXPLICIT"` (`type: "USER_INPUT"`), whereas spawned subagent conversations begin at `step_index: 0` with `source: "SYSTEM"` (`type: "SYSTEM_MESSAGE"`).
- `USER_INPUT` records wrap user text in `<USER_REQUEST>\n...\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\n...`.
- `PLANNER_RESPONSE` records carry private `"thinking"`, assistant `"content"`, and `"tool_calls"` (`[{"name", "args"}]`); subsequent `MODEL` `GENERIC` steps record each tool invocation's returned output in FIFO order, using `"status": "RUNNING"` when a shell command detaches into a background task after returning its task handle.

## Decision

1. Allow `"antigravity"` in `runtime_api::conversation` and Android's `supportsConversationView`.
2. In `TerminalView::runtime_conversation_source`, validate `identity.session_id` as a strict single-segment ASCII identifier (`valid_antigravity_session_id`, rejecting `.`, `..`, leading dots, and path separators) without touching disk or spawning guest processes on the UI thread.
3. Resolve `<HOME>/.gemini/antigravity-cli/brain/<session_id>/.system_generated/logs/transcript_full.jsonl` (falling back to `transcript.jsonl` only when `transcript_full.jsonl` is absent) off-thread inside the owning environment (local host `home_dir()`, WSL guest `$HOME`, or authenticated SSH `~`), rejecting symlinks and Windows reparse points on every intermediate segment and opening leaf files without following symlinks (`O_NOFOLLOW` / `FILE_FLAG_OPEN_REPARSE_POINT`).
4. Enforce ownership in `validate_source` by checking both the 4-segment suffix (`<session_id>/.system_generated/logs/transcript[_full].jsonl`) and a non-`SYSTEM` first step so subagent transcripts fail closed.
5. Project `USER_INPUT` by unwrapping only the outer `<USER_REQUEST>...</USER_REQUEST>` envelope (preserving literal inner tags and repeated identical user messages), `PLANNER_RESPONSE` by emitting assistant prose and FIFO tool calls while omitting `"thinking"`, and `MODEL` `GENERIC` steps as completed tool outputs while hiding `SYSTEM` steps.

## Rejected alternatives

- Trusting an arbitrary hook-supplied `session_file` for Antigravity: unnecessary because the brain layout is fixed per `HOME` and `conversationId`, and deriving the path in the owning environment prevents local hook writers from redirecting reads outside `.gemini/antigravity-cli/brain/`.
- Treating `GENERIC` records with `"status": "RUNNING"` as incomplete tool calls: Antigravity writes `"RUNNING"` once when a backgrounded command returns its task handle and never rewrites that step, which would leave finished historical turns permanently marked in progress.
- Splitting user prompts on the first `</USER_REQUEST>` substring: truncates prompts that quote literal `<USER_REQUEST>...</USER_REQUEST>` examples.

## Consequences

- Mobile Conversation View renders Antigravity user messages, assistant replies, and tool calls/outputs across local, WSL, and SSH panes, and submits replies through the existing foreground paste barrier.
- If `transcript_full.jsonl` appears while `transcript.jsonl` is being read, capture reports `conversation_changed` so the client refreshes onto the untruncated file.

## Validation

- `assistant_answer::conversation::antigravity::tests` covers `<USER_REQUEST>` unwrapping with nested tags, `"thinking"` and `SYSTEM` exclusion, FIFO tool pairing, detached `"RUNNING"` completion, subagent rejection, path traversal rejection, and symlink fail-closed behavior.
- `platform::local_paths::tests::open_regular_file_no_follow_accepts_plain_files_and_rejects_symlinks`, `runtime_api::tests`, and Android `DesktopRecoveryTest` cover transport and UI gates.

## Supersedes

None

## Revisit when

Antigravity changes its brain transcript path layout, adds native tool-call correlation IDs across `PLANNER_RESPONSE` and `GENERIC` steps, or reports an authenticated transcript handle in its hook protocol.
