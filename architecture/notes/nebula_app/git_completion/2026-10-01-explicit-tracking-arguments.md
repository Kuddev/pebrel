# Explicit Git tracking arguments

## Status

Implemented; validation results accompany the change.

## Context

Automatic remote-branch guessing does not describe explicit tracking. `--track`
has an optional attached mode, the next word is a start point, and an omitted
new-branch name is derived from the spelling of that start point. Treating these
positions as ordinary branch switches or option values offers invalid edits.

## Evidence

- [Git switch](https://git-scm.com/docs/git-switch) and
  [Git branch](https://git-scm.com/docs/git-branch) describe direct tracking,
  inherited tracking and `--no-track`.
- Git 2.50.1 `builtin/checkout.c` derives an omitted name by stripping `refs/`,
  then `remotes/`, then the first slash-delimited component. This also applies
  to explicit `--no-track`; it is not automatic `--guess` behavior.
- Native Git 2.50.1 probes confirmed that an attached `--track=inherit` is a
  mode, while a separate `direct` is a positional argument. Explicit direct
  tracking rejects a remote reference excluded by its fetch configuration.
- `branch.c::setup_tracking` rejects duplicate remote destination mappings;
  `checkout.defaultRemote` does not disambiguate explicit direct tracking.

## Decision

Keep the tracking mode and new-name/start-point roles in the pure semantic
context. Add an explicit source variant to the existing application request
dispatch, with the same local-only admission and shell-aware replacement spans.

Derive direct-track eligibility from the existing bounded configuration/ref
snapshot. Offer local branches or unambiguous configured fetch destinations,
not arbitrary tags or unmapped remote references. Inferred names must be valid
literal branch names and must not already exist as local branches. An explicit
new name retains the full start-point choice for the selected tracking mode.

Inherited and disabled modes retain Git's commit-ish start-point behavior.
Inherited mode does not fabricate upstream configuration when the chosen source
has none; Git owns the actual branch configuration after explicit submission.

Reuse the current two-process metadata query, total output/deadline budget,
cache, invalidation and cancellation. Prefix filtering adds no process or I/O.
Accepting a candidate only edits input; execution still requires submission.

## Rejected alternatives

- Treat `--track` as a required-value option: consumes the start point as a mode.
- Reuse automatic guessed branch names: loses explicit remote selection and the
  difference between `checkout.defaultRemote` guessing and direct tracking.
- Execute one Git validation command per candidate: adds subprocess latency to
  prefix matching for facts already available in the repository snapshot.
- Put tracking rules in the terminal view: duplicates source knowledge across
  UI modes and moves it toward core input ownership.

## Consequences

No dependency, persistence, thread, permission or terminal ownership changes.
Host metadata remains unavailable to SSH, WSL and nested foreign-shell inputs.
This is not complete Git grammar or middle-of-line shell editing support.

## Validation

Existing semantic fixtures cover three shell syntaxes, optional mode values,
option order, new-name positions and unsupported argument combinations.
Application fixtures cover actual Git upstream results, direct/inherit/disabled
modes, exclusions, collisions, cache invalidation, three presentation modes,
closed-quote edits and foreign-filesystem isolation. Their execution results are
reported separately from native window/PTY acceptance.

## Supersedes

Extends `2026-09-30-automatic-tracking-branches.md` for explicit tracking.
The existing request ownership, query bounds and automatic-guess rules remain.

## Revisit when

Git changes tracking resolution, additional argument forms need this source, or
measured repository metadata costs justify changing snapshot construction.
