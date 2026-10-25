# Keep the 2.2.2 desktop-only release complete and fail closed

## Status

Accepted, 2026-10-16. Version 2.2.2 updates the desktop applications; Android
Preview remains at the published 2.2.1 version.

## Context

The stable asset contract normally includes the Android Preview APK and its
offline relay kit. Requiring these unchanged mobile assets for 2.2.2 would create
an unrelated mobile release. Removing them only from an upload command would
instead contradict the manifest, checksum validation and release documentation.

The reusable Android job is also an aggregate dependency. GitHub Actions applies
an implicit status check to job conditions, so skipping that job can skip desktop
aggregation and publication unless their conditions handle the dependency result.

## Evidence

- [The canonical stable manifest](../../../scripts/stable_release.py) owns exact
  asset sets, including the historical Windows ARM64 and mobile boundaries.
- [The stable workflow](../../../.github/workflows/release.yml) derives its Android
  requirement from that manifest. Desktop conformance reports are independent of
  Android evidence, and the full five-platform native workflow is still required.
- GitHub documents [dependency skip propagation](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idneeds)
  and the [implicit status check](https://docs.github.com/en/actions/reference/workflows-and-actions/expressions#status-check-functions).
- Android versionCode 23 and versionName 2.2.1-preview are preserved in the mobile
  manifest; desktop product versions are 2.2.2.

## Decision

Record 2.2.2 as a desktop-only exception in the versioned stable manifest. Its
nine product assets cover Windows x64 and ARM64, Linux, and both macOS
architectures. SHA256SUMS is an additional public asset. Historical asset sets
remain unchanged, and subsequent mobile-capable releases retain APK and relay
validation.

Prepare emits an explicit string output derived from the manifest. Android runs
only when that output is true. Aggregation accepts either required Android
success or an explicitly unneeded Android skip, while requiring every desktop
package job to succeed. Empty or unexpected output values fail closed.

Aggregation and publication use a cancellation-aware status condition to handle
the skipped ancestor. Publication still requires successful preparation, full
native tests and asset aggregation. Android evidence and the APK upload argument
are conditional; exact desktop assets, conformance evidence and checksums remain
mandatory.

## Rejected alternatives

- Re-upload an unchanged APK under the new desktop version: this would misstate
  the mobile update and require an unnecessary version increment.
- Ignore missing mobile files in the validator: an incomplete required release
  could then pass without its mobile evidence.
- Accept any skipped Android job: a release that requires Android could publish
  after an accidental skip.
- Use an unconditional aggregate or publication job: this could continue after a
  failed desktop dependency or cancellation.
- Disable Android for all later versions: the decision is scoped to 2.2.2.

## Consequences

The 2.2.2 release contains no APK or relay kit. Android users retain the existing
2.2.1 release. The desktop applications receive the shared GPUI improvements;
their required native and packaged-runtime validation is unchanged.

## Validation

The existing stable-release and Android-release test modules cover the exact
nine-asset manifest, missing desktop assets, unexpected mobile assets, checksum
names, historical and subsequent mobile manifests, and Android helper rejection
for 2.2.2. Workflow-condition tests evaluate the actual aggregate and publication
guards against success, skipped, failed, canceled and invalid-output cases.

The focused release, Android, branding and native-test orchestration unit suites
passed locally. These contract tests do not substitute for actual native runner
execution and final package evidence.

## Supersedes

None. Historical release contracts remain unchanged.

## Revisit when

A later version has an independently approved mobile update or a different
distribution scope. Keep its declared asset set and dependency conditions aligned
without retroactively changing published releases.
