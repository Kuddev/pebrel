# Native Windows ARM64 portable release

## Status

Implemented for the 1.9.0 release candidate, following the maintainer's request
to include Windows ARM builds in release CI. Public release remains pending.

## Context

The native test matrix already includes Windows ARM64, while stable release
packaging previously produced only Windows x64 binaries. Passing ARM64 tests
did not give ARM users a downloadable native application.

## Evidence

- `linux-lua.yml` runs tests on `windows-11-arm` and checks the compiler host.
- `package-release.ps1` previously fixed the archive suffix to `windows-x64`.
- The existing console-runtime preparation script has pinned ARM64 payloads.
- The installer and automatic installer selection currently target Windows x64.

## Decision

Add a native ARM64 portable ZIP to the stable asset manifest from 1.9.0 onward.
Build the application and Hook helper on the ARM64 runner with the shared
product build script. Package the corresponding pinned ARM64 console runtime.
Verify PE machine headers for all four executable payloads before assigning
the requested architecture to the ZIP filename.

The full release aggregate requires both the existing native ARM64 test job and
an ARM64 conformance report from the same source commit. Historical release
manifests and the separate Preview workflow retain their current contracts.

Keep the Windows installer and automatic installation on x64 for this release.
The ARM64 artifact is explicitly described as a portable ZIP; an ARM64 installer
requires its own native installation, migration and update-contract validation.

## Rejected alternatives

- Renaming an x64 archive would not provide native ARM64 execution.
- Cross-compilation alone would not exercise native startup and terminal behavior.
- Treating the native test job as packaging evidence would leave downloadable
  payloads and architecture mismatches unverified.

## Consequences

The 1.9.0 stable manifest contains eight packages plus SHA256SUMS. The additional
release job owns its runner, architecture-specific build cache and evidence.
No installer compatibility alias or historical asset is removed.

## Validation

- Package-header tests accept both supported machine types and reject a wrong
  label, mismatched Hook helper and truncated executable.
- Release-manifest tests require the ARM64 ZIP for 1.9.0 and retain old manifests.
- Evidence tests reject missing or mislabeled ARM64 reports when ARM64 is required.
- Native release CI is required before attaching candidate assets to the draft.

## Supersedes

None.

## Revisit when

A native ARM64 installer and its update/migration behavior have been implemented
and verified, or the supported Windows release architectures change.
