# Pebrel Android connection preview

This is the first native connection implementation, under development. It shares
Pebrel's desktop palettes. Its terminal state is now the public Ghostty VT core,
with a Pebrel JNI adapter and an Android hardware Canvas view.

## Build

Use Linux x86_64, JDK 17, Gradle 8.13, Android SDK 35, CMake 3.22.1 and NDK
27.2.12479018, Rust 1.97.1 and its Android targets. From the repository root:

```sh
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.2.12479018"
rustup toolchain install 1.97.1 --profile minimal --target aarch64-linux-android,x86_64-linux-android
export RUSTUP_TOOLCHAIN=1.97.1
python3 mobile/tools/build_russh.py
python3 mobile/tools/build_ghostty.py
cd mobile/android
gradle :app:testPreviewUnitTest :app:assemblePreview :app:assemblePreviewAndroidTest :app:lintPreview
```

The source archive, Zig compiler and Android NDK are pinned in
`android/ghostty/UPSTREAM.json`; the builder verifies hashes and preserves native
licenses. CMake links the resulting arm64-v8a/x86_64 VT cores into
`libpebrel_ghostty.so`. The package contains no private precompiled binaries or
desktop UI.
The `Mobile connection preview` workflow runs the build, native instrumentation
and UI interactions, and uploads APKs, screenshots, checksums and reports.
For a user-requested APK-only delivery, dispatch it with `package_only=true`;
this assembles and signs the APK without running the test or emulator jobs.

The Pebrel preview uses `io.github.kuddev.pebrel.mobile.preview`. It can coexist
with earlier preview packages; Android keeps their saved profiles separate.
`android/pebrel-preview.p12` is an intentionally **public development key**, password
`android`, alias `pebrel-preview`. It gives test builds a stable install identity;
it must never sign production releases. The release build type does not use it.
Source is available at https://github.com/Kuddev/pebrel at the artifact's recorded commit.

## Connections in this implementation

- Local: Android `/system/bin/sh` and available system tools. This does not ship
  an additional package environment or Git/Python/Node.
- SSH: password authentication, explicit first host-key approval, pinned host
  identity, encrypted PTY, native direct input and a locally edited command box.
  Saved host metadata contains no password. Candidate taps only edit the draft.
- Computer: an SSH server under the same account as the running Pebrel desktop,
  with a build containing `pebrel mobile-bridge` on PATH. Lists existing panes,
  subscribes to semantic task state, reads a bounded output tail and optionally
  sends validated prompts. The CLI bridge defaults to read-only. This is shared
  input, not exclusive takeover; coloured desktop grid streaming is not present.
- Notifications: Android local notifications for observed live desktop task
  transitions. No proprietary server, push service or durable replay is claimed.
- The foreground connection service is opt-in. Android can still terminate it;
  remote process survival requires the server's own session host, such as tmux.

The native interface now follows the approved local HTML page structure: session
thumbnail gallery, host search/edit, computer panes, compact command composer and
grouped settings. Thumbnail text is a bounded capture of a real terminal when the
gallery opens; thumbnails do not run hidden render loops. Command candidates only
fill the per-session draft. Font family/size, cursor shape/blinking, pinch zoom, suggestion visibility and
default input mode are persisted as display preferences, separate from credentials.
QR pairing, durable notification recovery, file/media transfer, WebDAV and
structured reading remain subsequent work. Local and SSH terminals use Ghostty;
the desktop bridge still supplies bounded text at approximately two-second intervals.

The launcher and home icon use the desktop Titanium asset. Other UI icons are Android vectors;
no new icon library or web runtime is needed. Compose owns page navigation and
display preferences; terminal rendering remains in a single attached native view.
Display preferences use Android SharedPreferences. Fonts are bundled Maple Mono
NF CN and JetBrains Mono, plus system monospace. Pinch gestures update terminal
geometry and save the final size. Cursor blinking stops with the view lifecycle.
Settings categories share a single rounded surface with internal dividers;
navigation uses directional transitions that respect the system animation setting.
The terminal uses the selected theme color without a decorative image background.

## Ownership and performance decisions

- Compose owns navigation and metadata. It never creates a composable per cell.
  The application owns terminal sessions; an Activity only attaches its view.
- Ghostty owns VT parsing, grid, scrollback, width calculation and terminal-mode
  key encoding. `SessionTransport` supplies local PTY or SSH bytes.
- Parser state and snapshots run on a serial coroutine dispatcher, separate from
  UI and blocking I/O. Reads use 16 KiB batches and suspend until parsing finishes.
  Input has a 128 KiB budget and 64-item bound; rejection preserves command drafts.
- JNI transfers immutable changed rows as UTF-16 strings and flat integer arrays.
  Android's text shaping, font fallback and HWUI glyph cache render visible cells.
  Compose never holds cells, and there is no custom EGL context or second parser.
- Redraws are event-driven and coalesced over 16 ms while a view is attached.
  Scrollback is bounded at 2,000 rows. No hidden thumbnail renderer or idle timer
  runs for hidden views. Cursor blinking uses a visible-view timer. These budgets
  do not establish a measured performance guarantee.
- The Android adapter provides local IME preedit, committed UTF-8, hardware keys,
  bracketed paste and viewport selection/copy. Long-press selects a line; drag
  extends selection. Image graphics and mouse reporting are not implemented.
- Local command editing is immediate on the device. Direct terminal interaction
  still includes SSH round-trip time; shell passwords and full-screen programs
  do not receive speculative local echo.
- Android uses the same pinned russh 0.62.2 as the desktop, through the independent
  `mobile/ssh` crate and the `:ssh` JNI module. The desktop retains command authority.
  The first bridge reuses authenticated SSH exec and the
  loopback API instead of adding a listener or distributing the runtime token.
- The bridge binds to one runtime endpoint per channel, accepts only an allowlist,
  requires explicit window and pane IDs, and bounds requests/responses at 40 KiB /
  2 MiB. It never replays prompts after disconnect. `mobile.ready` declares absent
  features explicitly. A future paired transport must preserve those boundaries.
- Desktop output requests carry target identity and generation. Host persistence
  is serialized. Neither late output nor an older save may overwrite newer state.
- Native visible text uses Android resources (English and Simplified Chinese);
  terminal output is not translated. Theme assets derive from `nebula_settings`,
  retaining the desktop as the single palette authority.

See `android/ghostty/UPSTREAM.json` for current native provenance and
`android/third_party/THIRD-PARTY-NOTICES.md` for licensing.

## Verification boundary

Compilation, unit tests and lint run in CI. Device acceptance still needs Chinese
IME, external keyboard, repeated connection/close, idle/burst output, background
transitions, packet loss and multi-host checks. CI success alone does not establish
real-device latency, battery life, reliable offline delivery or visual acceptance.

## User-hosted relay test kit

See [relay/README.md](relay/README.md) for Docker/Caddy deployment and the outbound
Windows/Linux/macOS connector. The APK accepts an invitation generated by that
kit. Relay TLS terminates at the user-owned server; this preview does not provide
end-to-end encryption. Mobile role credentials cannot register as the desktop.
Both Node and Rust transport adapters share the method allowlist in
`protocol/bridge-policy.json`; the desktop Runtime remains the validator and owner
of actual operations. The Android RPC client is shared by SSH and relay.

### SSH connection feedback and host metadata

The application owns connection stages and the lifetime of host-key confirmation;
russh emits progress at actual network, verification, authentication and channel
operations. The native transport runs on two shared Tokio workers; blocking JNI
stream calls run on the existing Android IO dispatcher. Request success is
acknowledged before the shell is shown as ready. See [transport design](ssh/README.md).
Closing a session cancels its pending fingerprint decision. A failed connection
shows a localized error category. Retrying requires an explicit connect action;
uncertain command input is never replayed.

Saved hosts add optional `icon` and `group` fields to the existing JSON record.
Older records default to `term` and `development`; identity trust is still tied to
address and port. The icon catalog is generated from the desktop authority, and
host grouping is an application preference, independent of the SSH transport.
Password authentication is implemented. Key-file and jump-host controls remain
visibly unavailable until those transports exist.

The host form accepts a password directly and provides Save and Save & connect.
The Save password choice uses independent AES-GCM records backed by Android
Keystore. Records are bound to the profile ID, address, port and username; changing
the login endpoint never reuses its previous secret. Keeping the password field
blank retains an existing credential, while Remove saved password deletes it
explicitly. Storage runs on the IO dispatcher and the form closes only after the
requested write succeeds. Passwords stay out of saved UI state and host metadata.
Authentication and grouping
use compact pill selectors. Hosts, computers and terminal previews use thin
outlined containers, with live connection status and no fabricated session data.
