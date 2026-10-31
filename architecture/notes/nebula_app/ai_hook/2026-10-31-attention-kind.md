# Structured attention details

## Status

Proposed for maintainer review.

## Context

Issue [#326](https://github.com/Kuddev/pebrel/issues/326) requests a way to
distinguish an approval prompt from a plan or question menu. Runtime clients
currently see the same `task_state: attention` for each interaction.

## Evidence

The hook parser already distinguishes `PermissionRequest`, Claude
`AskUserQuestion`, and Codex `request_user_input`, but normalizes each to
`NeedsAttention`. `AgentActivity` owns acceptance and lifetime; RuntimeAgent
previously projected only the coarse status and its evidence source.

## Decision

Keep the lifecycle state and wait protocol unchanged. Add optional
`agent.attention_kind`, classified from structured hook fields at normalization
and carried by the existing lifecycle. The installed OpenCode bridge emits
attention only for permission requests. Notifications reuse one exact vocabulary
for both admission and classification.

Only the active attention state exposes the detail. Tool completion, submission,
command end and a failed pane suppress it. Rejected foreign events cannot replace
it. Session compaction preserves an existing interaction. Screen-only and OSC 7501
attention expose `unknown`: their coarse state does not prove a question subtype.
Clients must also accept a missing field from older servers.

## Rejected alternatives

- New `task_state` variants would change existing wait and scheduling semantics.
- Parsing permission words from messages or selected text confuses quoted output
  with a live request. Unclassified evidence stays unknown.
- Storing details separately in UI adapters would duplicate acceptance and cleanup.
- Treating the detail as permission to answer would confuse observation with user
  authorization; the field is descriptive only.

## Consequences

The API adds no new storage, dependencies or background tasks. Snapshot projection
reads one enum; it does not scan the terminal or copy hook context. Existing
ordering also fingerprints the enum so different interaction kinds are distinct.
Classification coverage is limited to supported structured provider events.

## Validation

Parser tests cover permission, plan confirmation, choice menus, free input,
legacy ambiguity and bypass mode. Lifecycle tests cover foreign events,
compaction, resolution, command boundaries and screen fallback. Runtime tests
cover optional JSON compatibility, schema vocabulary, and the GPUI projection
from accepted hooks through failure and resolution. Execution results are recorded
in the pull request.

## Supersedes

None.

## Revisit when

Providers or OSC 7501 supply stronger interaction metadata, or a versioned API
needs richer choices and request identity. Extend the normalized facts and their
tests before adding another projection or prose classifier.
