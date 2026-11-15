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
  `~/.gemini/antigravity-cli/brain/<conversationId>/.system_generated/logs/transcript.jsonl`
  and `transcript_full.jsonl` under mutex.
- Root interactive conversations record step `0` with `"source":"USER_EXPLICIT"` (later
  steps in a root conversation may use `"source":"SYSTEM"` on background task wakeups, so
  only step `0` is invariant). Un-forked subagents start with step `0` `"source":"SYSTEM"`.
- Forked subagents (`swarm` `fork`) copy the parent's step `0`, and `ProtoStoreSubagentHub`
  synchronously writes `<brain>/<parentId>/.system_generated/subagents/<childId>.json`
  before starting the child.

## Decision

Gate Antigravity envelopes in
[`protocol.rs`](../../../../nebula_app/src/ai_hook/protocol.rs) via tri-state
[`classify_antigravity_envelope`](../../../../nebula_app/src/ai_hook/native_events.rs)
(`Primary`, `Subagent`, `Unverified`) before event ordering or pane routing, accepting only
`AntigravityVerdict::Primary`:

- Reject immediately as `Subagent` on explicit subagent fields (`parentConversationId`,
  `callerContext == "subagent"`, or `rootConversationId != conversationId`) on both local
  and remote transports so upstream payload fixes work without disk reads.
- Remote SSH envelopes (`parse_remote_envelope`) never inspect local host files; local
  envelopes that omit both `artifactDirectoryPath` and `transcriptPath` remain `Primary`.
- On local envelopes carrying path metadata, restrict paths lexically to the CLI's single
  trusted root `<home>/.gemini/antigravity-cli/brain/<conversationId>` (shared with
  [`local::antigravity::directory`](../../../../nebula_app/src/ai_hook/local/antigravity.rs))
  without `.` or `..` components.
- Enforce no-symlink / no-reparse-point policy across the trusted `brain` directory itself,
  all of its ancestors, every relative subdirectory, and the leaf file
  ([`regular_file.rs`](../../../../nebula_app/src/platform/regular_file.rs): ancestor
  `symlink_metadata` check plus Unix `O_DIRECTORY | O_NOFOLLOW` `openat` + `fstatat` +
  `O_NONBLOCK | O_NOFOLLOW`, and Windows segment `symlink_metadata` +
  `FILE_FLAG_OPEN_REPARSE_POINT` + `FILE_TYPE_DISK`).
- Execute disk probes on a bounded worker (`sync_channel(8)`, `200 ms` listener wait cap).
  Timeouts, full queues, unreadable/symlinked files, `read_dir` / metadata errors, or scans
  exceeding `4096` entries / deadline yield `Unverified` and are dropped instead of being
  treated as `Primary`.
- Cache positive `Subagent` verdicts permanently (64-entry ring); for non-`SYSTEM` step-`0`
  conversations, cache only the immutable step-`0` check while re-verifying parent subagent
  records on subsequent hooks so delayed or previously unreadable parent records still
  upgrade the conversation to `Subagent`.
- Guard mid-turn foreground ownership in
  [`AgentActivity::accepts_hook`](../../../../nebula_app/src/ai_hook/lifecycle.rs): while an
  Antigravity turn is active (`Working` or `Blocked`), a same-PID `SessionStart` carrying a
  different `session_id` cannot replace `owner.session` even if its parent subagent record is
  not yet readable on disk; `/new` switches remain allowed once the turn ends (`Idle`/`Done`).
- Hold an RAII `AntigravityForegroundLease` on accepted local Antigravity `Owner`s so an
  already-confirmed foreground conversation validates known parent records without re-scanning
  `read_dir(brain_root)`, allowing its subsequent `done` hooks to succeed even after `brain`
  grows past `4,096` directories while keeping unknown conversations bounded and fail-closed.

## Rejected alternatives

- Filtering in `nebula_hook`: violates the helper's zero-JSON, zero-dependency contract.
- Falling back `unwrap_or(false)` on probe timeout or caching negative subagent results:
  allows busy workers or late-visible parent records to leak child hooks into the foreground
  pane.
- Treating `agentName` or empty `lastUserInput` as subagent markers: `agy --agent <name>`
  sets `agentName` on foreground root sessions, and image-only prompts leave `lastUserInput`
  empty.

## Consequences

No new dependencies, persisted schema changes or GPUI-thread file I/O (`parse_envelope`
runs on the background pipe/socket listener thread). Unverified local path-bearing envelopes
fail closed rather than overwriting foreground session ownership.

## Validation

Unit regressions in
[`native_events/subagent_tests.rs`](../../../../nebula_app/src/ai_hook/native_events/subagent_tests.rs) verify
un-forked and forked subagents (cold-start, parent cache eviction, delayed parent record
visibility), full `parse_envelope` + `AgentActivity` preservation under probe timeout, full
worker queue and anomalous directory metadata, trusted-root and ancestor symlink rejection,
FIFO non-blocking behavior, and remote envelope isolation.

## Supersedes

None.

## Revisit when

Antigravity preserves `parentConversationId` or `callerContext` in external hook JSON across
supported CLI versions, or changes its local `~/.gemini/antigravity-cli/brain` layout.
