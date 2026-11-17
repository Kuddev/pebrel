# Requested 2.4.0 build and publication

## Status

Implemented for the maintainer's explicitly requested 2.4.0 release.

## Context

The maintainer requested a complete 2.4.0 release after reviewing a Windows
candidate, with the preceding release's bilingual notes and contributor format.
The maintainer also explicitly requested that regression suites not be run in
this session. The existing stable workflow couples builds to native suites and
conformance reports; the Android collector also requires executed test reports.

## Evidence

- The existing [stable workflow](../../../.github/workflows/release.yml) requires
  native suites, conformance results and Android test evidence before publishing.
- [Stable asset validation](../../../scripts/stable_release.py) owns the complete
  platform set, package names, file signatures, checksums and public Release checks.
- [Android validation](../../../scripts/android_release.py) separately exposes
  package identity and signing checks; native payload and relay verifiers operate
  on the actual built APK.
- The 2.4.0 version change requires fresh product builds. The existing 2.3.2
  Windows candidate cannot supply the 2.4.0 assets.

## Decision

- Add a workflow scoped to the `release/2.4.0-20261102` branch and the exact
  `2.4.0` request in `packaging/release-2.4.0.json`.
- Build the real GPUI application, console entry, hook and native mobile payloads
  with the pinned production toolchains. Reuse the existing package builders,
  freshness checks, architecture checks and Preview signing identity.
- Build mode uploads the complete packages and a source receipt without
  publishing. Each package's size and SHA256 are bound to the source commit and
  Actions run. The receipt explicitly says that regression suites were not run.
- Android collection reuses identity, signature, alignment, ABI, JNI, license and
  embedded relay checks. It does not manufacture unit or instrumentation results
  or call the existing collector with fictional reports.
- After artifact review, commit the final notes and a publish request naming the
  verified build run and exact source commit. Publication rejects intervening
  product/workflow changes; only the request, notes and Changelog may differ.
- The publisher verifies the successful run's repository, workflow, branch and
  source, the artifact receipt, every package and the final notes. It publishes
  with that job's `GITHUB_TOKEN`, then checks the real tag, public state, body,
  asset set, labels, sizes and GitHub digests through the existing stable helper.
- Only the publisher gets `contents: write`. Its token-created Release/tag does
  not recursively launch the standard tag-triggered regression workflow.
- The existing stable, native and Android workflows remain unchanged. This
  request does not waive protected-branch review or confer an administrator merge
  bypass for subsequent issue work.

## Rejected alternatives

- Relabeling old binaries would misrepresent their compiled version and source.
- Supplying synthetic passing conformance or test reports would conceal the
  validation that was not performed.
- Weakening the existing workflows would change ordinary release/PR policy beyond
  this maintainer request.
- Moving the built tag to a later notes-only commit would break source provenance.

## Consequences

This publication provides actual fresh package builds and artifact verification.
It provides no new regression-suite, desktop interaction, physical-device or
complete CLI-version coverage. The platform Preview designations remain intact.
The build receipt and Actions output preserve the actual validation boundary.
An existing public release can only be accepted when all existing metadata and
assets match; the publisher cannot replace assets or move a mismatched tag.

## Validation

Preparation validates the version, notes and Changelog. The workflow verifies
native architecture, packaged versions and executable payloads. Android checks
the compiled manifest, established certificate, native libraries and same-source
relay. Collection validates the complete package set, archive CRCs and checksums.
Publication checks the downloaded receipt and the resulting public Release.
Actual build results belong to the corresponding Actions run and source receipt;
this note does not report unexecuted suites as passed.

## Supersedes

None.

## Revisit when

The requested 2.4.0 release is complete. Future releases use the ordinary workflow
unless the maintainer explicitly requests another scoped publication policy.
