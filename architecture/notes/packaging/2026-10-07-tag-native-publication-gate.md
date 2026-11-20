# Same-commit native validation before tagged publication

## Status

Implemented for the maintainer's requested tag-first, full-CI automatic release
flow. Remote execution remains to be verified on the release tag.

## Context

The previous release workflow relied on native validation from main and PR runs.
Its own dependency graph covered packages and runtime conformance, not the full
native suites. The shared native workflow does not trigger on tag pushes.

The maintainer now requests creating the release tag first, running full CI for
that commit, then automatically publishing. A new tagged release commit can contain
version, source and workflow changes that no prior main run tested.

## Evidence

Before this change, release publication depended on prepare and aggregate only.
A tag on an untested off-main commit could reach publication after package checks;
the release graph contained no dependency on the five-platform native result.
The existing workflow contract test explicitly required this absence, so it did
not protect the newly requested tag-first case.

## Decision

Call the existing local reusable Full native tests workflow from release.yml.
The local workflow reference and default checkout bind validation to the same
release commit; do not recreate its matrix, test selection or runner commands.
Publication requires prepare, native-tests and aggregate to succeed. Native tests
and package builds may proceed in parallel; failed tests prevent publication.

Retain Windows-only diagnostic packaging without this additional call: that mode
already verifies its selected source's Windows result and never enters publication.
Grant the reusable job only its existing contents/actions read permissions. The
publish job alone retains contents write permission.

## Rejected alternatives

- Assume a green main/PR run covers a later tag's different commit.
- Trigger a separate native run without binding publication to its result.
- Duplicate the five-platform matrix and slowly diverge from the shared owner.
- Continue publication after a failed, canceled or skipped full-release test job.

## Consequences

An explicit release validates its exact tagged source even if a prior PR tested
similar code. This additional release-time execution is deliberate for the requested
ordering. Existing PR/main validation and package/runtime evidence are unchanged.
Full publication can finish only after both testing and packaging are complete.

## Validation

The existing stable workflow contract now checks the reusable call and publication
dependency. Negative mutations remove the native prerequisite, substitute another
workflow, disable the call or ignore errors; each is rejected. Existing matrix
checks still require all five native hosts and both macOS release checks for full
callers. No assertions, native suites or package checks were removed.

## Supersedes

The package-only release assumption encoded by the former
test_stable_workflow_packages_without_repeating_native_tests contract, specifically
for the maintainer's newly requested tag-first automatic publication flow.

## Revisit when

Publication is changed to require an independently recorded successful native run
for the exact immutable tagged commit; such a design must reject other commits and
failed or incomplete runs before removing this dependency.
