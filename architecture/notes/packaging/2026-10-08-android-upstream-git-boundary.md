# Android upstream Git discovery boundary

## Status

Implemented; native package validation is recorded by the release workflow.

## Context

The Android builder extracts the pinned terminal core below the application
checkout. The archive has no `.git` directory. A tagged application checkout can
therefore affect upstream version detection even when every downloaded byte and
toolchain pin is unchanged.

## Evidence

- Stable run [37643642046](https://github.com/Kuddev/pebrel/actions/runs/37643642046)
  failed in the first Android native build before APK compilation or tests.
- Pinned upstream `28f9367bee11ad42f40f8aa589eb8c6db62d34be` calls `git -C
  <build-root> rev-parse`, `log`, and `describe --exact-match --tags` in
  `src/build/GitVersion.zig`.
  Its `Config.zig` rejects a discovered release tag that differs from its own
  version. The enclosing application tag is not upstream version evidence.
- [Git documents](https://git-scm.com/docs/git#Documentation/git.txt-GITCEILINGDIRECTORIES)
  a discovery ceiling, while an explicit `GIT_DIR` takes precedence.

Upstream attribution: [terminal core version detection](https://github.com/ghostty-org/ghostty/blob/28f9367bee11ad42f40f8aa589eb8c6db62d34be/src/build/GitVersion.zig).

## Decision

The terminal builder supplies a child-process environment that stops repository
discovery at the extracted source parent and removes inherited explicit Git
directory overrides. The application checkout and its release tag remain intact.

Android packaging also accepts a manually selected, exact source commit. Both
relay architectures and the APK check out and verify that commit; asset evidence
records the selected source rather than the commit containing the workflow.
The native-build step carries the same discovery ceiling for historical source
recovery, because an old build script cannot contain its later repair.

## Rejected alternatives

- Repeating the same failed tagged build preserves the deterministic failure.
- Moving a release tag would change the identity of already verified assets.
- Assigning the application version to upstream would misrepresent its identity.
- Omitting Android tests or mixing assets from different source commits would
  violate the existing source and artifact evidence checks.

## Consequences

An archive build uses the upstream fallback for a source tree without Git
metadata. Source pins, archive checksums, library version, compiler, Android ABIs,
signing, unit tests, lint and instrumented acceptance remain unchanged.
Manual Android recovery produces artifacts and evidence; public publication still
requires validating the complete same-source asset set and native test results.

## Validation

The existing Android release tests reproduce parent-tag discovery in a real Git
repository, then verify isolation with and without explicit inherited Git paths.
They also preserve the application tag and check workflow source/evidence binding.
Cloud APK builds and emulator tests remain the native acceptance evidence.

## Supersedes

None.

## Revisit when

The pinned upstream no longer probes Git outside its source archive, or Android
packaging gains a shared same-source recovery entry point.
