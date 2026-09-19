# Pebrel Android third-party notices

Pebrel Android combines GPLv3-compatible components. The application is
distributed under GPL version 3. Original component licenses and copyright
notices remain applicable; inclusion does not relicense upstream sources.

- Ghostty libghostty-vt, revision `28f9367bee11ad42f40f8aa589eb8c6db62d34be`,
  https://github.com/ghostty-org/ghostty, MIT. Built from verified public source
  with Zig 0.15.2; native dependency notices and build provenance are bundled
  under assets/licenses/Ghostty and Ghostty-UPSTREAM.json.
- AndroidX / Jetpack Compose and Kotlin / kotlinx libraries: Apache License 2.0.
- russh 0.62.2: https://github.com/warp-tech/russh, Apache License 2.0.
  The independent Rust transport is built from public source using ring and RSA
  support. JNI, Tokio, ring and the other resolved dependencies retain their own
  licenses; texts, Cargo.lock, dependency list and provenance are bundled under
  assets/licenses/Russh.
- OkHttp / Okio: https://github.com/square/okhttp and https://github.com/square/okio,
  Apache License 2.0.
- ZXing Android Embedded 4.3.0 and ZXing Core 3.5.3:
  https://github.com/journeyapps/zxing-android-embedded and
  https://github.com/zxing/zxing, Apache License 2.0. Used only for local QR
  invitation capture and decoding; no Google Play service is required.

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
