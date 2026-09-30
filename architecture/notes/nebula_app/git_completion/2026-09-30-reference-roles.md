# Match Git references to the argument's role

## Status

Implemented; acceptance evidence is recorded with the change.

## Context

The application read only local branches even where Git accepts a commit-ish:
new-branch start points, detached checkout, merge and rebase. Simply adding every
tag and remote-tracking ref to that list would make ordinary `git switch` offer
arguments it rejects. Ambiguous short names could also select the wrong object
or unexpectedly detach HEAD during a normal checkout.

## Evidence

- Git's [switch documentation](https://git-scm.com/docs/git-switch) distinguishes
  a branch from the start point used by `-c`, `-C` and `--detach`.
- [for-each-ref](https://git-scm.com/docs/git-for-each-ref) supplies non-ambiguous
  short names, worktree occupancy, symbolic targets and peeled object types.
  A native Git probe confirmed nested annotated tags peel to `commit`.
- A native repository with both a branch and tag named `same` returns
  `heads/same` and `tags/same` as strict short names. Ordinary `switch same`
  selects the branch; `checkout heads/same` uses revision semantics.

## Decision

Keep argument roles in `nebula-completions::semantic`. The existing branch source
remains branch-only; revision sources admit commit-like branches, tags and
remote-tracking refs. Checkout's initial argument keeps its path alternative.
Attached option values expose the same value prefix to matching and I/O projection.

Extend the existing per-pane Git snapshot to hold those reference names in one
bounded `for-each-ref` invocation. Ask Git for strict short names even when the
repository disables ambiguous-ref warnings. Ordinary branch selection preserves
the actual branch name; revision positions use the disambiguated name. An explicit
`refs/` prefix keeps full names, including inside `--onto=...`.

Only admit objects whose direct or peeled type is `commit`; tree/blob tags are
not valid targets for these currently supported revision positions. Exclude remote
symbolic aliases such as `origin/HEAD`. Worktree occupancy continues to exclude
busy branch-switch targets without excluding valid start-point references.

Keep the existing output bound, cancellation, two-second cache and invalidation
generation. Parse metadata without allocating a temporary vector per row. No
network request, shell completion invocation, package or polling worker is added.
Disable lazy fetching and set an empty `GIT_ALLOW_PROTOCOL` allowlist in the query
process. The latter also blocks transport access on older Git versions that do
not understand `GIT_NO_LAZY_FETCH`; incomplete local object data can fail the query
without contacting a promisor remote.

## Rejected alternatives

- Offer all reference kinds everywhere: breaks normal branch-only switch.
- Strip `refs/heads/` from every reference: loses tag and remote namespaces.
- Use disambiguated revision names for default branch checkout: may detach HEAD.
- Read `.git/refs` directly: misses packed refs and duplicates Git's worktree and
  reference rules.
- Query one subprocess per tag or prefix: increases input latency and duplicates
  the existing snapshot lifetime.

## Consequences

Windows, macOS and Linux share the same source and semantic rules. Remote-tracking
refs mean refs already stored in the local repository; this does not fetch remotes
or enable host Git I/O for SSH/WSL/nested-shell inputs.

Default remote-branch guessing and its configured refspec/defaultRemote policy,
tracking options, arbitrary revision expressions and additional Git subcommands
remain separate extensions. Unsupported grammar retains the existing fallback.

## Validation

The real Git fixture covers lightweight, annotated and nested tags; non-commit
tags; remote symbolic aliases; ambiguous names with warnings disabled; explicit
full refs and attached option values; packed refs and deletion/invalidation.
Native desktop acceptance adds tag start points in all three modes, a remote-ref
start point and detached HEAD. It verifies acceptance does not execute and an
explicit Enter produces the expected repository state.

## Supersedes

Extends `../completion/2026-09-30-semantic-arguments-and-scripts.md` and
`2026-09-30-local-branch-context.md`; request ownership and source admission remain
in `../completion/2026-09-30-request-boundary.md`.

## Revisit when

Another argument accepts tree/blob revisions, remote guessing needs configured
refspec resolution, or measured repository size makes this metadata query too costly.
