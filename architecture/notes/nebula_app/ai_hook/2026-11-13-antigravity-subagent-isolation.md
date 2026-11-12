# Antigravity in-process subagent hook isolation

## Status

Implemented on 2026-11-13.

## Context

Antigravity CLI (`agy`) executes subagents (`invoke_subagent` and `swarm` workers) inside
the primary CLI OS process and fires the same global `SessionStart`, `PreInvocation` and
`Stop` hooks for each child conversation. Because both conversations share one `agent_pid`,
[`AgentActivity::accepts_hook`](../../../../nebula_app/src/ai_hook/lifecycle.rs) treated a
subagent `SessionStart` as a foreground `/new` reset, overwriting the pane's active
`conversationId`, cold-restore resume target (`agy --conversation <id>`), session history
and turn state while dropping subsequent hooks from the real foreground conversation.

## Evidence

- In Antigravity CLI, `HookArgsCommon` populates `parentConversationId`,
  `rootConversationId` and `callerContext` internally, but `jsonhook.marshalHookArgs` scrubs
  internal fields before serializing hook stdin JSON
  ([upstream #1527](https://github.com/google-antigravity/antigravity-cli/issues/1527)),
  leaving external payloads with only the child `conversationId`, `transcriptPath` and
  `artifactDirectoryPath`.
- `SessionStart` runs as a `PreInvocationHook` after step `0` is synchronously flushed to
  `<appDataDir>/brain/<conversationId>/.system_generated/logs/transcript.jsonl` and
  `transcript_full.jsonl` under mutex.
- Root interactive conversations record step `0` with `"source":"USER_EXPLICIT"` (later
  steps in a root conversation may use `"source":"SYSTEM"` on background task wakeups, so
  only step `0` is invariant). Un-forked subagents start with step `0` `"source":"SYSTEM"`.
- Forked subagents (`swarm` `fork`) copy the parent's step `0`, and `ProtoStoreSubagentHub`
  synchronously writes `<brain>/<parentId>/.system_generated/subagents/<childId>.json`
  before starting the child. The parent's own `PreInvocation` hook fires immediately before
  the tool call that spawns the fork.

## Decision

Filter Antigravity subagent envelopes in
[`protocol.rs`](../../../../nebula_app/src/ai_hook/protocol.rs) via
[`is_antigravity_subagent`](../../../../nebula_app/src/ai_hook/native_events.rs) before
event ordering or pane routing, matching the existing native Codex `agent_id` gate:

- Reject immediately on explicit subagent fields (`parentConversationId`,
  `callerContext == "subagent"`, or `rootConversationId != conversationId`) on both local
  and remote transports so upstream payload fixes work without disk reads.
- On local pipe/socket envelopes only, validate an absolute, traversal-free
  `brain/<conversationId>` path matching `conversationId`, bound the step-`0` read to one
  line (`256 KiB` cap), reject `"source":"SYSTEM"` and check `.system_generated/subagents/`
  under known conversations in the same `brain` directory for forked children.
- Cache validated conversation paths in a bounded 64-entry map so subsequent
  `PreInvocation` and `Stop` hooks perform zero disk I/O. Remote SSH envelopes never read
  local host files.

## Rejected alternatives

- Filtering in `nebula_hook`: violates the helper's zero-JSON, zero-dependency contract.
- Treating `agentName` or empty `lastUserInput` as subagent markers: `agy --agent <name>`
  sets `agentName` on foreground root sessions, and image-only initial prompts leave
  `lastUserInput` empty.
- Blocking all same-PID `SessionStart` transitions for Antigravity: breaks legitimate
  foreground conversation switches inside one CLI process.

## Consequences

No new dependencies, persisted schema changes or GPUI-thread file I/O (`parse_envelope`
runs on the background pipe/socket listener thread). Payloads without a valid local `brain`
path fall back to accepting the conversation unless explicit subagent markers are present.

## Validation

Unit regressions in [`native_events.rs`](../../../../nebula_app/src/ai_hook/native_events.rs)
verify un-forked and forked in-process subagents never hijack an active turn or saved
resume command, subsequent root conversations still switch cleanly, explicit markers are
rejected on local and remote paths, `agentName` and path-traversal inputs do not misfire,
and `parse_remote_envelope` never inspects local host files (`151 passed, 1 ignored` in
the `ai_hook` suite).

## Supersedes

None.

## Revisit when

Antigravity preserves `parentConversationId` or `callerContext` in external hook JSON across
supported CLI versions, or changes its local `brain/<conversationId>` transcript layout.
