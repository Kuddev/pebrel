# Rename a whole source root without resetting its contracts

## Status

Accepted with the maintainer-requested Pebrel naming migration, 2026-11-09.

## Context

The maintainer asked for the remaining `nebula` names, including crate source
directories such as `nebula_app`, to become `pebrel`. The
[completion package identity note](../../nebula-completions/2026-09-30-package-identity.md)
already recorded that the line-budget checker rejects removed source roots,
so a physical directory rename needed a reviewed policy change first.

## Evidence

Against the base revision, replacing `root nebula_app` with `root pebrel_app`
failed with "source roots may not be removed from the budget". Without that
rule, a rename could also drop files silently, and legacy allowances under the
new path would look like new exceptions or lose their base-size bound.

## Decision

`architecture/file-budgets.txt` accepts `moved <old-root> <new-root>`. With a
base revision the checker lists the base tree under the old root and requires
each source file there at the same relative path under the new root. The old
root must not exist, the new root must not already be a base root, and
allowances under the new root are compared with the base allowance and base
size of the old path. Files inside a moved root are renamed in a separate
change.

A move only applies when the base still has the old root. After the rename is
merged the record has no effect and the checker prints a removal notice, so later
PRs are not blocked and their allowances are not remapped.

## Rejected alternatives

- Inferring moves from Git rename detection: similarity thresholds depend on
  content edits made in the same change and are not reviewable as policy.
- Allowing root removal whenever a new root appears: coverage could shrink.
- Keeping old directory names: contradicts the requested naming migration.

## Consequences

Directory renames are explicit in the budget file and checked offline. One
extra base-tree listing runs only for active move records.

## Validation

`scripts/tests/test_architecture_budgets.py` covers a legitimate move, dropped
or renamed files, a leftover old root, missing base listing, preserved and
increased allowances, claiming an existing root, stale records after merge and
malformed records.

## Supersedes

None.

## Revisit when

Source roots stop mapping to directories, or a root must be split into several.
