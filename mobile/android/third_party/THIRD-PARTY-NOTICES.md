# Pebrel Android third-party notices

Pebrel Android combines GPLv3-compatible components. The application is
distributed under GPL version 3. Original component licenses and copyright
notices remain applicable; inclusion does not relicense upstream sources.

- Ghostty libghostty-vt, revision `28f9367bee11ad42f40f8aa589eb8c6db62d34be`,
  https://github.com/ghostty-org/ghostty, MIT. Built from verified public source
  with Zig 0.15.2; native dependency notices and build provenance are bundled
  under assets/licenses/Ghostty and Ghostty-UPSTREAM.json.
- AndroidX / Jetpack Compose and Kotlin / kotlinx libraries: Apache License 2.0.
- Google Material Icons, outlined `settings` (24px): Apache License 2.0.
  Source: https://github.com/google/material-design-icons/blob/master/src/action/settings/materialiconsoutlined/24px.svg.
  Converted to Android VectorDrawable without changing the icon geometry;
  the Apache-2.0 license is included in licenses/Apache-2.0.txt.
- russh 0.62.2: https://github.com/warp-tech/russh, Apache License 2.0.
  The independent Rust transport is built from public source using ring and RSA
  support. JNI, Tokio, ring and the other resolved dependencies retain their own
  licenses; texts, Cargo.lock, dependency list and provenance are bundled under
  assets/licenses/Russh.
- OkHttp / Okio: https://github.com/square/okhttp and https://github.com/square/okio,
  Apache License 2.0.

Dependency coordinates and versions are recorded in Gradle files. CI publishes
the runtime dependency report with the build evidence. Dependencies may include
additional notices in their own archives; do not strip those notices at packaging.

Full GPLv3 and Apache-2.0 license texts accompany this notice in licenses/.

Maple Mono NF CN 7.900: Copyright 2022 The Maple Mono Project Authors
(https://github.com/subframe7536/maple-font), SIL Open Font License 1.1.
The unmodified desktop font is bundled; see licenses/MapleMono-OFL.txt.

JetBrains Mono 2.304: Copyright 2020 The JetBrains Mono Project Authors
(https://github.com/JetBrains/JetBrainsMono), SIL Open Font License 1.1.
The unmodified regular font is bundled; see licenses/JetBrainsMono-OFL.txt.
Exact font source files and hashes are recorded in FONTS.json.

## QR scanning

ZXing Android Embedded 4.3.0 (Apache-2.0), pinned Maven dependency `com.journeyapps:zxing-android-embedded:4.3.0`.
Uses on-device camera decoding without a remote recognition service. License from upstream commit `24d02945fec5f2c5a65b24ea7848cb5ca18f9f81`, included as `licenses/ZXing-Android-Embedded-Apache-2.0.txt`.

ZXing Core 3.4.1 is the scanner decoder dependency (Apache-2.0).
Source: `https://raw.githubusercontent.com/zxing/zxing/272d9561b547c82670918790c34b490da1aec3b0/LICENSE`.
License text: `licenses/ZXing-Core-Apache-2.0.txt`; SHA256 `3f62881f0566227a24b12e5a754cc79f39aaa94883038e95c94812e1f50af42f`.
