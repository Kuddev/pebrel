# Same-version repair is not identified by a changed main binary

## Status

Implemented; pending review.

## Context

[Issue #258](https://github.com/Kuddev/pebrel/issues/258) supplies an Inno log
reporting successful installation and the expected Pebrel version, while the
update helper reports that the same-version binary did not change.

## Evidence

The reported preceding update replaced the main executable but failed with a
locked Hook helper. Reinstalling the same package repairs other files while
leaving the main executable byte-identical. Its hash cannot distinguish that
successful repair from an installation which did no useful work.

## Decision

Retain package SHA-256/size checks, commit authority, participant identity and
exit waiting, installer exit-code checks, and the installed version probe.
Remove the additional requirement that a same-version main binary change hash.
Record the installed hash as evidence, not as a repair-success predicate.
Before starting setup, retry exclusive write access to installed Hook helper files
for five seconds. Short-lived forwarding can finish; persistent locks fail before
any installation file is replaced. The helper never kills unrelated processes.

## Rejected alternatives

- Suppressing every installer error would hide partial/failed installations.
- Forcing a main-binary byte difference does not validate the repaired files.
- Disabling same-version retries prevents recovery after a partial upgrade.

## Consequences

Installer success and the expected application version determine success for a
verified package. The updater does not independently audit every installed file;
installer errors continue to fail the transaction. No installed version tag or
asset naming rule changes.

## Validation

The native handoff suite passes twelve scenarios, including a byte-identical main
binary with a repaired helper. The former same-version/no-change assertion was
incorrect; its negative intent is retained as `upgrade-noop`, which returns
installer success but leaves the wrong application version and must still fail.
Cancellation, checksum/identity failures and unprepared processes remain covered.
A released helper-file lock permits installation; a held lock must prevent setup
from starting and recover the unchanged original application.

## Supersedes

The same-version changed-hash assertion in the handoff helper and its old fixture.

## Revisit when

A versioned installed-file manifest provides a stronger package-completeness
check without confusing unchanged files with failed installation.
