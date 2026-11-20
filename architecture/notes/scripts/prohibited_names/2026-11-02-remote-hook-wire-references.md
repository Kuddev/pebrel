# Remote Hook wire references remain distinct from product comparisons

## Status

Proposed for maintainer review before main-branch integration.

## Context

The maintainer requested remote persistent-session Agent support and removal of
competitor comparisons. The name gate already preserves exact compatibility
references in the remote-session adapter, but not the new Hook delivery adapter.

## Evidence

The checker at `620cccd9` rejects the five precise source expressions recorded
in `test_desktop_hook_wire_references_preserve_only_the_exact_interface` in
`scripts/tests/test_prohibited_names.py`: the environment lookup, private route
directory, and matching scope-header encoder/parser in the remote Hook adapter.
Those expressions identify actual transport interfaces. Removing their names
would change protocol behavior without removing any comparative copy.

## Decision

Recognize only the exact environment getter, directory expression and matching
wire-header fragments in their three owning files. Continue scanning the rest
of every line, comments, labels, other files and commit messages. No directory
or new source file is excluded from the gate.

## Rejected alternatives

- Renaming real interfaces solely to satisfy a text filter.
- Exempting whole source files, documentation paths or all command strings.
- Accepting the same syntax in unrelated prose or with an appended comparison.

## Consequences

Compatibility keeps its existing meaning. Ordinary comparison wording remains
prohibited. The added fixtures pair each accepted expression with rejected
same-line commentary, unrelated paths and standalone product labels.

## Validation

The source expressions above supply reproducible false positives against the
baseline checker. The actual name check is rerun on the reviewed candidate
history. Regression test cases are
updated but their suite is not executed, following the user's instruction.

## Supersedes

None; extends the existing exact-interface recognition to its new adapter.

## Revisit when

The bridge's wire fields, private storage expression or module ownership changes.
