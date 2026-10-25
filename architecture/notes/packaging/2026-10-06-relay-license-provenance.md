# Source-bound offline relay notices

## Status
Maintainer repair for the offline installation path reported in #486.

## Context
The manual-kit packer referenced installer files absent from both main and the
v2.1.1 tag. Restoring those files exposed a second distribution gap: the packer
carried binaries without the product license or their native dependency notices.

## Evidence
The official v2.1.1 APK digest is
`91ab48c2655a02fac906f2fd29772d89c6e96d6bc9912da4dc0a439cbed45165`.
Its Android transport notice inventory has 237 dependencies but does not contain
the relay's direct axum, rustls or rcgen entries. Its tokio notice is for 1.53.1;
the relay workspace resolves tokio 1.50.0. Reusing that inventory by package name
would therefore omit dependencies and misidentify versions.

`mobile/link/Cargo.toml` declares the relay feature and `Cargo.lock` pins its
dependency versions. An initial workspace metadata probe included 171/170
dependencies and activated unrelated endpoint/preview features. It would also
require unrelated workspace sources in a cold packaging cache. Package-selected
`cargo tree` with the actual relay feature, target and normal/build edges resolves
118/117 dependencies without those extra features.

## Decision
Extract the existing transport license-text collection into `cargo_notices.py`.
Keep its record format, authors, origin, declared texts and verified standard
Apache fallback. Retain Cargo's historical slash-separated alternatives; do not
replace an AND/WITH obligation with a single fallback license.

`relay_notices.py` binds an inventory to an exact source checkout and Linux target.
Cargo selects the component's normal/build dependency closure. The collector reads
the corresponding cached registry archives only after their checksums match the
source lockfile, retains manifest attribution and license files, then hashes the
notice inventory. It never unpacks arbitrary archive paths. Development-only and
unrelated packages are excluded. No handwritten crate allowlist decides coverage.
Collection is offline after dependency archives are available. The current graph
is registry-only; a future path/Git dependency requires explicit source handling,
not silently substituting another package with the same name and version.

Native packaging stores notices beside each architecture's binary. APK release
collection requires them; historical binary-only APK verification remains an
explicitly narrower operation. Manual kits always require verified notice bundles,
either in the APK or supplied from its matching source checkout for an older APK.
They also retain the product GPL text. Notice readers share one path/hash/size
validation implementation and never extract paths from an archive onto disk.

Android release collection generates the kit from the APK whose identity and
tests were verified, and binds the archive digest into that release evidence.
The shared stable manifest requires one universal kit from 2.1.2 onward; published
2.1.1 and older asset sets retain their historical contract. The existing artifact
and global SHA256SUMS pipeline carries it without a second build matrix. Local
per-kit checksum sidecars remain staging outputs, not extra public assets.

## Rejected alternatives
- Android SSH notices are a different graph and version set.
- Copying a generic MIT/Apache text without package attribution loses provenance.
- Maintaining a second copy of the transport collector lets license choices drift.
- Workspace-wide metadata expands the selected feature set and can require
  unrelated downloads; use the same package selection as the binary build.
- Requiring old public APKs to contain newly introduced entries retroactively
  breaks historical verification; supplied source-bound notices repair their kits.
- Downloading replacement relay binaries changes the code users intended to install.

## Consequences
Only build/distribution artifacts change. No runtime dependency, network request,
worker, private-key storage or application hot-path work is added. Each target
carries a bounded, potentially over-inclusive notice set; its compressed size is
measured separately from application memory. Missing texts, mismatched sources,
wrong targets, escaped paths and damaged notices stop packaging.

## Validation
The existing APK/installer tests cover complete kits, old APKs with supplied notices,
missing notices, source/target/hash mismatch, path traversal and shared collection
fallbacks. Real collection resolves 118 x86_64 and 117 aarch64 dependencies in the
selected relay build. Service installation tests use a recording fixture; they do
not establish a real Linux installation or physical Android-device acceptance.
The extracted transport collector retains its 237 dependency records and 462
text files. Collection from the exact v2.1.1 checkout produces matching notices
for the official APK; the resulting licensed manual kit is about 3.5 MB. These
are archive measurements, not an application RSS or hot-path benchmark.
Public Release assets remain unchanged until publication is separately selected.

## Supersedes
None.

## Revisit when
The relay feature graph, license expressions, Cargo tree output or registry archive
layout changes. Recollect against the actual binary source rather than copying a
previous version's inventory or broadening a failed check.
