# Remote endpoint metadata is a compatibility interface

## Status

Proposed for maintainer review with the remote session recovery change.

## Context

The prohibited-name gate already distinguishes remote CLI commands and typed
session tags from promotional prose. Named-socket recovery additionally needs
executable defaults, process-name comparisons, executable-path matching, and
isolated server lifecycle tests. The old rules rejected these required interfaces.

## Evidence

The new positive cases in `test_remote_endpoint_metadata_and_isolated_recovery_commands_are_scoped`
fail against the prior checker, including the typed executable default, exact
process identity comparison, and an isolated server existence query. Existing
session commands are already recognized by `remote_reference_remainder`.

## Decision

Extend that same authority with exact endpoint/process-reference forms in their
owning connection files. Allow isolated recovery fixture commands only in their
owning test file and only for the supported actions. Remove just the matched
interface fragments; continue checking all remaining text on each line.

## Rejected alternatives

- Exempting a directory, file, all strings, or all comments would admit promotion.
- Altering or constructing executable names solely to evade the checker would
  hide real compatibility dependencies and harm reviewability.
- Allowing general product mentions in commits or prose is not needed here.

## Consequences

Executable/process interfaces can be maintained without broadening editorial
permissions. Unknown commands, ordinary labels, copied references in a README,
and promotional text appended to otherwise valid code remain rejected.

## Validation

Positive and negative tests exercise the observed false positives, wrong paths,
unknown commands, ordinary labels, and same-line appended product mentions. Run
the existing prohibited-name and architecture governance suites unchanged.

## Supersedes

None; extends existing remote compatibility references.

## Revisit when

The remote metadata protocol changes or another language adapter owns these
interfaces; add examples and counterexamples before expanding recognized syntax.
