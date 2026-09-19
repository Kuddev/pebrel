# Android russh transport

This independent Rust crate uses the same pinned russh 0.62.2 and ring/RSA
features as the desktop. It does not depend on desktop UI, Android rendering or
Ghostty. Kotlin keeps host metadata, UI, explicit trust decisions and application
session ownership. JNI carries control events and bounded byte chunks only.

A process-wide Tokio runtime has two workers. Each connection owns one SSH
session and one shell or exec channel; there is no implicit connection pooling or
automatic input replay. Numeric JNI handles index an owned registry rather than
exposing raw pointers. Closing removes the handle and cancels pending DNS,
connect, trust, authentication, channel and byte-stream work. In-flight JNI calls
hold an Arc and wake on cancellation; a stale handle cannot access freed memory.

Queues are bounded: 16 control events, 16 input chunks of at most 8 KiB, and eight
16 KiB chunks for each output stream. Remote output backpressures the transport.
Shell stderr joins terminal output; exec stderr remains separate from RPC stdout.
PTY and shell/exec requests must receive success before the UI can become ready.
No rendering work or network work runs on Android's main thread.

Connection stages originate from actual operations. Host fingerprints use russh's
SHA256 OpenSSH encoding. A changed stored fingerprint is rejected, and a new
identity waits for the application's explicit confirmation with a 60-second
budget. Authentication first checks server-authorized none authentication, then
tries the supplied password once. Key-file and jump-host transports are not
implemented.
Password bytes are not persisted or logged; temporary bridge-owned copies are
zeroized, without claiming control over every internal library allocation.

`Cargo.lock` locks transitive dependencies. `mobile/tools/build_russh.py` compiles
both Android ABIs with the pinned NDK and 16 KiB page alignment, and bundles the
resolved dependency notices. Only the pinned open-source dependencies are linked;
no private implementation is included. `SshIntegrationTest` exercises the
optimized APK against a real OpenSSH fixture, including password rejection,
changed identity and declined trust.
