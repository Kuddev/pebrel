# Keep automatic package builds out of pull-request checks

## Status

Accepted by the maintainer on 2026-09-23: pull requests run tests and compile
checks; distribution packages are built after merge or on explicit request.

## Context

Preview packaging subscribed to pull requests that changed package inputs.
A Windows installer-menu change therefore started Linux and both macOS package
builds alongside the independent native test matrix. These workflows requested
the same macOS runner types while required checks were waiting for runners.

## Evidence

- [PR #264](https://github.com/Kuddev/pebrel/pull/264) changed Windows installer
  scripts and triggered [Preview run 35825643107](https://github.com/Kuddev/pebrel/actions/runs/35825643107).
- The Preview macOS jobs ran workspace tests, dialog tests and an optimized
  product build before creating DMGs. The native workflow independently ran
  workspace tests and checked the production feature graph.
- The required `Release workspace` jobs execute `cargo check --release`; they
  do not generate distribution packages and remain required compile checks.
- Runner waiting is observable. The exact hosted-runner quota or scheduling
  policy responsible for a particular delay is not established by these logs.

## Decision

Remove the `pull_request` trigger from `preview-packages.yml`. Keep its filtered
`main` push trigger and explicit `workflow_dispatch`, including all package and
runtime conformance validation. Public Preview publication still requires an
explicit dispatch with publication enabled. Stable release triggers are retained.

The native PR and merge-group workflows retain their test coverage. This change
does not alter required-check names or server-side protection rules.

## Rejected alternatives

- Cancel required macOS checks to merge sooner: loses required validation.
- Treat the release-profile compile checks as package creation: these commands
  validate a different build profile without creating installers or archives.
- Remove package conformance tests: package behavior still needs verification
  when a package is actually requested.

## Consequences

Pull requests no longer automatically provide Preview installation artifacts.
Maintainers can explicitly dispatch package validation for a candidate branch.
Already queued workflows must be handled separately; changing a trigger cannot
retroactively cancel them. This change does not remove duplicate native runs
when a draft becomes ready, or guarantee a fixed runner wait time.

## Validation

The native-suite workflow contract verifies that Preview has only filtered main
push and manual triggers, that stable release remains tag/manual, and that native
PR and merge-group checks remain unfiltered. Restoring the previous PR package
trigger must fail the new regression.

## Supersedes

The previous automatic Preview build policy for pull-request package-input changes.

## Revisit when

An explicit request for per-PR artifacts can be implemented with bounded platform
selection and a separate runner budget without delaying mandatory validation.
