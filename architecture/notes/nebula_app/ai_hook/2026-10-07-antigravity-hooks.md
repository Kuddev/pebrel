# Antigravity CLI hooks

## Status

Implemented on 2026-10-07. Validated by unit tests only; the CLI was not run against the
installed configuration (see Validation).

## Context

Antigravity CLI (`agy`) replaced Gemini CLI, which stopped serving free and personal
accounts on 2026-06-18. Its [hooks reference](https://www.antigravity.google/docs/hooks/)
defines a global `~/.gemini/config/hooks.json`, named groups with an `enabled` flag, and the
events `PreToolUse`, `PostToolUse`, `PreInvocation`, `PostInvocation` and `Stop`. The
payload carries `conversationId` but no event name.

## Evidence

- Captures of 1.2.7 reported by third parties
  ([capture](https://github.com/automatis-tools/agents-can-communicate/issues/177),
  [config](https://github.com/Digital-Process-Tools/claude-remember/issues/563),
  [paths](https://atamel.dev/posts/2026/07-16_where_agy_hooks/)) agree on flat
  `{"type","command","timeout"}` entries for lifecycle events, and show only `SessionStart`,
  `PreInvocation`, `PostInvocation` and `Stop` firing; tool events were silently dropped.
- `Stop` carries `terminationReason`, `error` and `fullyIdle`. Observed reasons are
  `NO_TOOL_CALL`, `model_stop`, `max_steps_exceeded` and `error`; the reference lists none.
- No event reports a permission wait.

## Decision

- Install one `pebrel` group
  ([`antigravity.rs`](../../../../nebula_app/src/ai_hook/local/antigravity.rs)) with
  `SessionStart`, `PreInvocation` and `Stop`. Each command passes its event as `--event`,
  the contract Grok, Copilot and Cursor already use, so the helper and parser stay shared.
- `PreInvocation` runs before every model call, so it reports activity (`tool-complete`),
  not a submitted prompt. `Stop` finishes the turn.
- A stop is classified only from `error` and the observed reasons. Any other reason leaves
  the pane idle rather than announcing completion, as for Cursor and Copilot.
- The helper prints `{}` for this source: Antigravity reads a hook's stdout as its decision.
- A group named `pebrel` that holds anything but our commands is never rewritten.

## Rejected alternatives

- Subscribing `PreToolUse` and `PostToolUse`: documented but not observed to fire, and they
  would spawn the helper per tool call.
- Reading `fullyIdle` as background work: its meaning is only inferred from captures.
- The Claude-style `settings.json` installer: the file, key layout and entry shape differ.

## Consequences

No attention events, so the shared lifecycle may use the existing screen observations for
permission prompts. Local installation only; resume and fork stay unsupported. Hooks stay
inert while the group's `enabled` flag is off or Antigravity does not load the file.

## Validation

Tests cover ownership of the named group, repair, removal, preservation of other groups
and of a same-named foreign group, malformed and busy files, the argv event contract, the
conversation id and stop classification. Native CI runs them on five platforms. Live
behavior, the Windows shell, and whether an empty `{}` is accepted on every event are
not verified here.

## Supersedes

None.

## Revisit when

Antigravity documents `terminationReason`, a permission event or session end; fires tool
events; or changes the global hook path or group schema.
