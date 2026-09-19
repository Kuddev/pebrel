# Termux terminal reuse

Source: https://github.com/termux/termux-app
Revision: `4584488513c099f2e98bcfcd00f006d213248729`
License: upstream declares GPL-3.0-only with Apache-2.0 exceptions for code
originating in Terminal Emulator for Android. The unmodified upstream declaration
is retained in LICENSE.md. Preserve the individual source headers as well.

Vendored terminal-emulator and terminal-view source, resources and tests retain
their upstream responsibilities: VT grid, drawing, IME, selection and gestures.
The combined Pebrel Android application is distributed under GPL version 3,
including corresponding source and build instructions. Apache-2.0 code retains
its original terms and notices; it is compatible with combination under GPLv3.
The upstream exception is not a blanket claim that every subsequent Termux change
in these directories is available under Apache-2.0 alone. We do not rely on such
a claim. Neither termux-app nor termux-shared is copied into this project.

License copies are in ../licenses/ and included in the APK assets. The Android
Terminal Emulator and AOSP attribution is included in THIRD-PARTY-NOTICES.md.

Local changes:

- Gradle library adapters target the mobile project and current NDK.
- TerminalSession accepts a SessionTransport. TerminalSessionClient has default
  transport readiness and rejected-input callbacks.
- The transport adapter owns bounded reader/writer queues. I/O and resize happen
  off the UI thread; VT is drained in bounded batches on the existing view owner.
- Android local PTY uses public ParcelFileDescriptor APIs, avoiding private
  FileDescriptor field access. It starts only /system/bin/sh; it does not include
  Termux bootstrap, package repositories or a general Linux distribution.

Upstream removal condition: a supported injected byte transport with equivalent
backpressure, cancellation and public descriptor handling. Upstream test sources
remain present; they must continue compiling in the Android CI job.
