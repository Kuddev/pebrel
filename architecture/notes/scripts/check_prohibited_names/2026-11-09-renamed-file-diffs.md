# Check only changed lines in renamed files

## Status

Accepted with the maintainer-requested Pebrel naming migration, 2026-11-09.

## Context

The naming migration moves whole crate directories. The prohibited-name gate
checks added lines, but listed changed files with `--name-only` and then diffed
each file with only its new path as the pathspec.

## Evidence

With only the new path, Git cannot pair the file with its old path, so every line
of a moved file appears added. Moving `nebula_app` to `pebrel_app` therefore
reported 82 unchanged lines, such as existing theme-format and keyboard-protocol
identifiers. A fixture that moves a file containing an existing comparison fails
the old range and staged checks for the same reason.

## Decision

List changes with `-M --name-status`. For a rename, diff with both the old and
new path as pathspecs so Git reports only the lines that changed; report the new
path. Ordinary commits in range and push mode and staged mode share this path.
Merge commits and root commits keep their existing diffs.

## Rejected alternatives

- Exempt moved directories: added text inside them would go unchecked.
- Skip pure renames only: a move with edits would still replay the whole file.
- Rely on the default `diff.renames`: a pathspec of one side defeats detection,
  and user configuration could disable it.

## Consequences

Below Git's default 50% similarity a move is still a delete plus an add, so the
whole file is checked, which fails closed. Lines that a move edits are checked as
new lines, including existing names on those lines.

## Validation

`scripts/tests/test_prohibited_names.py` moves a directory whose file holds an
existing comparison: staged and range checks pass. Adding a comparison in the
moved file fails both checks, reports the new path and omits the unchanged line.
Both tests fail against the previous implementation.

## Supersedes

None.

## Revisit when

Merge commits need rename-aware combined diffs.
