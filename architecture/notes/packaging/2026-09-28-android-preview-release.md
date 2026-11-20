# Android Preview in the desktop release transaction

## Status

Implemented and released in 2.0.0. Hosted build and x86_64 emulator validation passed;
physical-phone acceptance remains separate.

## Context

The 2.0 release includes an Android companion. Existing users have installed a
Preview application, so CI must not replace its package identity or signing key
with a newly generated debug/release identity. A desktop-only checksum manifest
would also omit the APK from the final publication checks.

## Evidence

- `mobile/android/app/build.gradle.kts` owns the Preview variant and its signing configuration.
- The current optimized Preview APK verifies with certificate SHA256
  `93587953edfe9611601768833b5f008ff700d85c2486ddcfb7f7ea82b2de827b`.
- Android Gradle Plugin 8.10 requires Gradle 8.11.1 and JDK 17:
  https://developer.android.com/build/releases/past-releases/agp-8-10-0-release-notes
- Android's 16 KiB alignment check uses `zipalign -c -P 16 4`:
  https://developer.android.com/tools/zipalign

## Decision

Keep the `.preview` application ID and current Preview certificate. Advance the
version code from 17 to 18 and use `2.0.0-preview` as the visible Android version.
This remains Preview distribution, not a production signing or Store release.

The release workflow calls one Android packaging workflow. Build both Android
ABIs and the two embedded Linux relay binaries from the same source commit.
Native musl relay builds retain systemd/OpenRC portability without substituting
old deployment assets. Reuse existing native composition and relay validators.

Run the complete debug unit-test suite and Preview lint, then exercise the optimized APK's terminal and
selected real OpenSSH/SFTP paths on an x86_64 emulator. Persistent-session tools
and optical QR pairing remain separate acceptance scenarios.

Compose JVM tests use the existing debug-only test Activity manifest. The first
hosted attempt ran them as Preview and 50 tests failed before UI assertions with
`Unable to resolve activity` for `androidx.activity.ComponentActivity`. Run the
same complete unit-test sources under debug; keep native instrumentation on the
optimized Preview APK. Adding the test Activity to the distributed Preview would
unnecessarily change its manifest.

Collect the APK only after checks pass. Verify the compiled package name,
incremented version, signer, archive alignment and both test reports. Bind the
evidence to the source commit and actual APK hash. The aggregate job requires
Android success and includes the APK in SHA256SUMS and publication verification.

## Rejected alternatives

- Generate a CI debug key: breaks update identity for the current Preview.
- Publish the unconfigured production variant: changes the package and signing contract.
- Upload an APK as optional evidence: permits a nominally successful release with no Android asset.
- Reuse a local APK or old embedded relay: breaks source-to-artifact traceability.
- Add Android work to every desktop test matrix: duplicates release work outside this request.

## Consequences

The current Preview identity is intentionally distinct from an older Preview
certificate. Retaining this identity establishes continuity with the current
Preview, not automatic migration from every historically distributed APK.
The shared release manifest requires Android from 2.0.0 onward and preserves
the exact historical desktop-only asset sets.

## Validation

Focused Python tests cover signer/package/version mismatches, missing or failed
tests, wrong source/hash evidence and a missing or malformed APK. The
[Android release job](https://github.com/Kuddev/pebrel/actions/runs/36452556963/job/109035735360)
passed 142 unit tests and 18 x86_64 emulator tests, then verified the Preview
identity, signer, native payload and source-bound evidence. The downloaded public
APK matches that evidence. Physical-phone coverage remains separate.

## Supersedes

None.

## Revisit when

Android moves to production signing, a Store channel or an explicit certificate
migration. Preserve installed-user identity deliberately rather than replacing
the key as an incidental packaging change.
