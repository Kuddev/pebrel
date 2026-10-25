# Remote control references in naming checks

## Status

Focused correction prepared for maintainer review after reproducing PR #712.

## Context

The naming policy preserves real compatibility references but rejects ordinary
product comparisons and promotional text. Mobile remote recovery extends the
existing discovery protocol with client history commands, pane selection and
typed consumers outside the original discovery file. The old recognizer does
not cover those interfaces and rejects their literal executable names and tags.

## Evidence

Running the range checker against `8d629559..69dc5c9b` reproduces the CI failure.
The added lines include optional Unicode CLI flags, client listing, history-mode
commands, enum-to-tag mappings and constructors used by reconnect tests.
`TmuxDiscoveryTest` also launches the real executable in an isolated environment;
`SshPtyIdentityTest` supplies realistic process-table records.

The range also contains ordinary prose, fixture labels, a local field name and
two commit subjects. Those are distinct from the mandatory interfaces and remain
rejected. The revised checker does not make that original range pass unchanged.

## Decision

Extend command recognition only for the concrete added CLI forms. Restrict
history commands to their controller and its tests, model tags to the existing
typed consumers and connection/session tests, and native-process setup/fixtures
to the responsible test sources. Match the enum and its corresponding tag as one
unit before removing the enum reference on its own.

Each pattern removes only the recognized occurrence. All remaining text on the
line is still checked, including additional names, comments and external URLs.
README text, technical-note prose, test descriptions and commit messages keep the
existing rules. Name fields by their responsibility, describe behavior in prose,
and refer to implementation types when a precise technical reference is needed.
No commit identities, accepted subjects, whole directories or whole files are
added to an exemption list; the prohibited-name list does not change.

## Rejected alternatives

- Remove or split real executable names and protocol tags: changes the interface
  or hides it from review without fixing the recognizer.
- Permit every literal or comment within the mobile tree: conceals unrelated
  product text and promotional destinations beside valid integration code.
- Exempt conventional fix subjects or all technical notes: subject formatting and
  directory location cannot establish the purpose of arbitrary prose.
- Relax the CI selector or bypass the failed check: neither repairs the policy.

## Consequences

The implementation can preserve literal remote commands and faithful process
fixtures. Incidental names still require editing before a clean replayed range
passes. Existing per-commit scanning still catches text added and later removed.
This change has no runtime product or Android packaging effect.

## Validation

The focused naming suite exercises the real new CLI, tag and fixture forms,
unsupported forms, unrelated paths and appended promotion. Staged and full-range
tests accept a real history command and reject an added external promotional URL.
Tests also retain the rejection of technical-note prose and fix subjects.
The original feature range is checked explicitly to distinguish repaired
compatibility references from remaining authoring changes.

All 26 naming tests and 13 architecture-governance tests pass. Loading the base
checker against the three new interface tests reproduces 37 failed subcases;
the updated checker passes them. The architecture checker against `8d629559`
and `git diff --check` also pass. The unchanged PR selector chooses only the
Linux native runner for this policy/test/note diff and no release-build runner.

## Supersedes

Extends the consumer and command coverage of
`2026-09-28-mobile-integration-references.md`; its exact-occurrence boundary stays
in force.

## Revisit when

Another real remote-control interface requires additional syntax. Supply a
reproduced failure, positive cases and appended/out-of-scope negative cases
before broadening the recognizer.
