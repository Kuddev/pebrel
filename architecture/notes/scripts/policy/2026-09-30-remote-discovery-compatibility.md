# Remote discovery compatibility references

## Status
Proposed for separate maintainer review.

## Context
The added-content name checker already recognizes mobile session startup and
stop commands. Remote discovery also needs executable probes, inventory queries,
attachment commands and stable wire tags.

## Evidence
PR #389 failed the name check on these actual interfaces. Replacing executable
names breaks remote invocation; replacing only parser tags breaks discovery.
The focused examples in scripts/tests/test_prohibited_names.py reproduce the
false positives against the previous checker.

## Decision
Extend the existing mobile Kotlin command recognition with specific command
prefixes. Recognize wire tags only at their constructor, lookup, comparison and
dispatch positions in the discovery implementation. Strip only those occurrences
and continue checking the remainder of each added line.

## Rejected alternatives
Do not exclude a file or directory, allow every quoted product name, alter the
actual commands, or permit arbitrary documentation and comparison text.
Incidental prose and local variable names are cleaned in the feature change.

## Consequences
Existing compatibility interfaces can be maintained without hiding names.
New commands or tag contexts still require explicit review. Commit messages and
the entire pending commit range retain their current checks.

## Validation
The checker regressions cover accepted commands/tags, unrelated files, unknown
commands, arbitrary labels, and appended comparisons using the same and other
product names.

## Supersedes
None.

## Revisit when
The remote interface changes, or a concrete false positive or false negative
shows these narrow contexts no longer describe the maintained integration.
