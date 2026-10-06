# Git metadata follows one captured execution scope

## Status

Implemented. The completion request owns source admission; the worker captures
an existing transport before consulting repository snapshots.

## Context

Git arguments need configured remote names and mapped references. A local Git
query cannot describe a WSL or SSH repository, and choosing a pooled connection
again for each read can combine different sessions after reconnection.

## Evidence

[`metadata.rs`](../../../../nebula_app/src/completion/metadata.rs) adapts the
captured process or authenticated transport.
[`ssh_session/completion.rs`](../../../../nebula_app/src/ssh_session/completion.rs)
retains a connection generation and an independently owned query channel.
Request and transport regressions exercise configured remotes, negative fetch
mappings, ambiguous routes, cancellation and connection generations.

## Decision

Read remote names from repository configuration and branch names from locally
available fetch mappings and refs. Do not fetch, authenticate or run queries in
the interactive PTY while completing an argument.

The worker freezes one existing SSH connection before source-cache lookup.
Missing or ambiguous routes decline the source. Reconnection creates a distinct
cache identity even when the destination spelling is unchanged. WSL retains its
selected distribution and captured user. Host reads remain host reads.

Query processes and channels have deadlines, output limits and cancellation.
Git prompt, optional-lock, lazy-fetch and allowed-protocol controls apply inside
the guest as well as to local children. Checkout path conflicts are checked on
the same machine. Configuration queries do not return remote URLs or credentials.

A failed query is unavailable, not a successful empty snapshot. Unavailable
foreign sources retain environment-scoped history without a host-filesystem
fallback. Valid empty metadata cannot revive deleted names through history.

## Rejected alternatives

- Use host paths for a remote command: equal spelling does not identify equal data.
- Authenticate during typing: changes ownership and can create interactive prompts.
- Re-select a connection for each read: can cross connection generations.
- Cache query failures as empty repositories: hides recovery until expiration.

## Consequences

No dependency or persisted format is introduced. Each pane retains bounded
repository snapshots with explicit invalidation. Destinations with multiple
active authenticated routes require an explicit connection owner before dynamic
metadata can be admitted. Unknown source roles retain their native fallback.

## Validation

Production request fixtures cover configured remote/refspec roles and exact
literal edits. Transport fixtures reject missing and ambiguous connections,
retain a captured generation, close cancelled channels and keep the shared
connection usable. Existing repository invalidation and worktree tests remain.

## Supersedes

Extends `2026-10-01-explicit-tracking-arguments.md` beyond local admission and
`../completion/2026-09-30-request-boundary.md` with a captured metadata transport.

## Revisit when

A pane supplies an explicit connection handle, another transport has an owned
metadata boundary, or a command requires independently validated remote discovery.
