# A generic notification cannot establish a blocking state

## Status

Accepted following the maintainer's requirement to map explicit client states.

## Context

The structured notification adapter accepted a missing notification type for old
bridges. The lifecycle admitted that event during an active turn. Ordinary text
notifications could consequently replace a running state with the waiting hand.

## Evidence

`payload::attention_is_actionable` previously returned true for an absent type.
`AgentActivity::accepts_hook` only rejected that fallback while idle or finished.
An active turn proves activity, not that a notification requests permission.

## Decision

Require a known actionable notification type. Ignore missing and unknown types
without inspecting message prose. The reducer also rejects legacy untyped
attention facts regardless of its current state. Explicit PermissionRequest,
question-tool hooks and the client's reserved attention title remain independent
sources of waiting-state evidence.

## Rejected alternatives

- Inferring a permission from the notification message or permission mode.
- Treating every notification during a running turn as blocking.

## Consequences

Old untyped notifications no longer change activity. The installed native hooks
and supported typed notification spellings keep their mapping. The Runtime
schema includes the distinct title source already emitted by native-title reads.

## Validation

Existing fixtures now express the explicit-type contract. Formatting, schema
parsing and architecture validation accompany the commit. No regression suite
was run, as requested; native product compilation is part of candidate packaging.

## Supersedes

The untyped-notification compatibility exception in the parser and reducer.

## Revisit when

A client documents another structured notification type or explicit state API.
