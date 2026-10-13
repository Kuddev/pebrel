# Complete automatically created tracking branches from repository facts

## Status

Implemented; acceptance evidence accompanies the change.

## Context

Ordinary switch previously offered only existing local branches. Git can also
create a local tracking branch from an unambiguous remote-tracking reference.
Removing the remote name from every reference would suggest commands that fail
when remotes disagree, another reference shadows the name, or tracking is disabled.

## Evidence

- [Git switch](https://git-scm.com/docs/git-switch) documents `--guess`,
  `checkout.guess`, `checkout.defaultRemote` and explicit tracking controls.
- Git's `checkout.c::unique_tracking_name` queries each remote's configured
  fetch mapping. `refspec_find_match` uses the first matching positive mapping;
  a missing target does not allow a later mapping to take its place.
- Native Git 2.50.1 probes confirmed that `--no-track` disables automatic branch
  creation and a same-named tag prevents ordinary switch from guessing a branch.
- That Git version can still guess a stale reference excluded by a negative
  fetch refspec. Such a reference is locally resolvable but is no longer a source
  selected for fetching by the user's configuration.

## Decision

Keep command flags in the pure semantic context and metadata interpretation in
the existing application Git source. A cohesive `tracking` child interprets
configuration, resolves literal and wildcard fetch mappings, and produces names
of branches eligible for automatic creation. It performs no I/O and owns no tasks.

Read only relevant configuration keys with NUL-separated `git config` output.
A private command-scoped sentinel makes absent configuration a successful query;
the query does not collect remote URLs or credentials. Read all local references
because configured fetch destinations can live outside `refs/remotes/`.

Both subprocesses share the existing three-second deadline, cancellation and
one-MiB total output budget. The per-pane snapshot caches configuration-derived
names with reference facts for two seconds and invalidates on command submission.
Prefix filtering does not start further Git subprocesses. No dependency or
long-lived worker is introduced; transport and lazy fetching remain disabled.

Count all resolvable matching destinations when evaluating ambiguity, including
destinations that are not usable commits. Otherwise, ignoring an unusable remote
could falsely make another remote appear unique. Offer a name only when its
unique or preferred destination is a usable non-symbolic commit reference.

Honor explicit negative fetch exclusions when selecting automatic suggestions.
Withhold an excluded name across remotes instead of accidentally converting a
real ambiguity into a unique candidate. Explicit revision completion still permits
the existing local reference. This is deliberately more selective than the stale
reference behavior observed in Git 2.50.1.

Keep existing local branches authoritative and avoid names already resolved by
other references. Checkout also rejects a guessed name colliding with a local
file; inspect only the at-most-256 matched candidates, not every known branch.
This exclusion is specific to automatic Git branch suggestions, not a claim that
every existing checkout path/reference ambiguity has been resolved.

## Rejected alternatives

- Strip a fixed `origin/` prefix: ignores custom refspecs, renamed remotes and
  default-remote selection.
- Read `.git/refs` or parse the configuration file directly: misses packed refs,
  worktrees, included configuration and Git's normal precedence rules.
- Probe each candidate with a subprocess: adds latency proportional to branch count.
- Start a persistent Git worker or duplicate rules per shell: no current lifecycle
  requirement justifies either cost.

## Consequences

Windows, macOS and Linux use the same implementation. Three completion modes keep
their existing input behavior. Local remote-tracking data does not enable network
fetching or local-host queries for SSH, WSL or nested foreign-shell contexts.

The first uncached query adds one bounded config process; measured native candidate
latency must accompany acceptance. Repository mutations outside the application
can remain visible only after the existing cache expiry. Explicit `--track`
argument completion, workspace selectors and additional commands remain extensions.

## Validation

Existing semantic tests cover flags in all three shell syntaxes. The real Git
fixture checks configured defaults, disabled guessing, negative exclusions,
non-commit ambiguity, literal/wildcard mappings, custom namespaces, first-match
ordering, Unicode, packed-ref deletion and actual upstream selection.

Native window/shell/PTY acceptance adds automatic tracking creation in all three
modes, Right acceptance, checkout, a preferred remote and a custom namespace.
Each case checks that accepting completion does not execute, then verifies the
branch and upstream after an explicit Enter.

## Supersedes

Extends `2026-09-30-reference-roles.md` for automatic tracking names and snapshot
contents. Existing request ownership and source admission remain unchanged.

## Revisit when

Explicit tracking options need a different argument source, Git changes its remote
resolution behavior, or measured repository sizes make the bounded config/ref
snapshot too costly.
