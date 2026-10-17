# Claude-style hook agents: Qoder, CodeBuddy, Qwen Code and Droid

## Status

Implemented on 2026-10-07. Validated by unit tests only; no provider CLI was run against
the installed configuration (see Validation).

## Context

Four agents document Claude Code's hook contract: JSON on stdin with `session_id` and
`hook_event_name`, `Stop`, `Notification` with `notification_type`, `UserPromptSubmit`,
`SessionStart` and `SessionEnd`, configured under a top-level `hooks` key of a user-level
`settings.json` (`~/.qoder`, `~/.codebuddy`, `~/.qwen`, `~/.factory`). Their official
references are [Qoder](https://docs.qoder.com/cli/hooks),
[CodeBuddy](https://www.workbuddy.ai/docs/cli/hooks),
[Qwen Code](https://qwenlm.github.io/qwen-code-docs/en/users/features/hooks/) and
[Droid](https://docs.factory.com/reference/hooks-reference.md). Other agents surveyed the
same day cannot supply a turn-end or attention event: Crush documents only `PreToolUse`
and Junie CLI only `SessionStart`, so they receive identity and icons but no hook.

## Evidence

- All four document timeouts in seconds and omit any mention of reading `~/.claude`.
- Droid documents no `PermissionRequest`, `StopFailure` or `PostToolUseFailure`.
- Qwen documents `CLAUDE_PROJECT_DIR` alongside its own variable; CodeBuddy requires
  review of externally edited hooks in its `/hooks` panel before they apply.
- Windows providers run hooks through different shells (CodeBuddy enforces Git Bash,
  Qwen documents PowerShell). Only Claude documents the exec form with `args`.

## Decision

- Each agent signs its own `source` and shares Claude's parsing branch
  ([`claude_style`](../../../../nebula_app/src/ai_hook/protocol.rs)); the session id still
  comes only from `session_id`, and a camelCase payload is rejected as another runner's.
- Installation reuses the shared-file pattern of Cursor
  ([`claude_style.rs`](../../../../nebula_app/src/ai_hook/local/claude_style.rs)): one pure
  edit that installs, removes and inspects, claiming only a complete helper invocation.
  The command is a single shell string (quoted POSIX, encoded PowerShell on Windows)
  shared with Grok and Cursor through `command_for` and `owns_command`.
- `PreToolUse` is not subscribed: nothing here reads it, and it would spawn the helper
  twice per tool call. `PostToolUse` alone returns a stale permission wait to working.
- The hooks are opt-in like every non-default agent; `ai_hooks_<agent>` stores the choice.

## Rejected alternatives

- The `args` exec form on Windows: undocumented outside Claude, and an ignored `args`
  leaves a bare helper path that silently forwards nothing.
- Adding `*_PROJECT_DIR` to the foreign-runner gate: those names are documented as hook
  variables, not as proof that no child shell inherited them. A false match would drop
  genuine Claude notifications without any error, while nothing shows these agents read
  `~/.claude/settings.json`.
- Subscribing every documented event: process cost with no consumer.

## Consequences

Local installation only; SSH and WSL installation does not cover these agents, as with
Kimi, Grok and Cursor. Resume and fork commands stay unsupported. A user-written
`disableAllHooks` (Qwen) or an unreviewed change (CodeBuddy) can make an installed hook
inert; the settings page reports file state, not provider activation.

## Validation

Unit tests cover installation, repair, removal, foreign-hook preservation, malformed and
busy files, cross-agent ownership, Droid's event subset and the parsed lifecycle. Native
CI runs them on five platforms. Live behavior of each CLI, including whether omitting
`matcher` fires tool events and how Windows shells run the encoded command, is not
verified here.

## Supersedes

None.

## Revisit when

A provider changes its hook schema or event names, documents reading another agent's
configuration, or ships an exec form that works on Windows; or when Crush or Junie add a
turn-end event.
