# Follow Droid's effective user hook configuration

## Status

Implemented for PR #518 review.

## Context

The integration originally always edited `~/.factory/settings.json`. Droid ignores
that fallback when its standalone hooks file exists, so inspection could claim a
successful installation that never receives events.

## Evidence

The [Factory reference](https://docs.factory.com/harness/hooks.md), checked on
2026-10-25, documents the native user `hooks.json`, legacy `hooks/hooks.json`, and
`settings.json` fallback. Standalone files map event names directly at the root.
An omitted matcher explicitly matches all tool names.

## Decision

Select the first existing native, legacy, or settings file, in that order. New
installations use the native file. Reuse the same owned-command editor with the
layout selected from the effective path. Preserve foreign hooks and do not create
a higher-precedence file that would hide existing user hooks.
Disabling removes owned entries from all three paths, including inactive fallback
entries that could otherwise reappear after a native file is removed.

## Rejected alternatives

- Always editing settings: silently ineffective when native hooks exist.
- Always creating a native file: masks foreign legacy/settings hooks.
- Migrating all user configuration: unnecessary ownership beyond our own entries.

## Consequences

No new setting or dependency. Inspection reports the effective file. Invalid or
busy files produce errors and remain untouched. Removal is not a multi-file
transaction; a failed removal remains retryable and is reported to the caller.

## Validation

Production editor tests cover native/settings coexistence, flat event schema,
legacy and settings precedence, repair, idempotence, removal and foreign entries.
The application hook suite passed 152 tests with one existing ignored test.
No live Droid CLI invocation was performed.

## Supersedes

The Droid configuration-path assumption in
[the original integration note](2026-10-07-claude-style-agents.md).

## Revisit when

Factory changes file precedence or the event schema.
