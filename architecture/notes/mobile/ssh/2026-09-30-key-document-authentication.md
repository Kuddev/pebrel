# Android key-document authentication ownership

## Status

Implemented; validated through the mobile unit and native/device acceptance suites.

## Context

The host form exposed a disabled key choice while the native transport only tried
none/password authentication. Enabling that control alone would not provide key login.

## Evidence

`HostForms.kt` selects a document; `SessionRepository` persists host metadata and
encrypted credentials; `SshKeyFile.kt` reads bounded bytes on the SSH IO worker.
`mobile/ssh/src/transport.rs` owns protocol authentication and host-key verification.

## Decision

- Reuse Android's document picker and persistable read grants, not a custom key vault
  or a plaintext private-key field. Save URI/name only after the user saves the host.
- Preserve the existing password credential identity. Key passphrases use an additional
  key-URI discriminator, so switching modes or documents cannot reuse an old password.
- Keep the original host-fingerprint checks. Explicit key mode uses public-key auth
  only; malformed, inaccessible or rejected keys never select password as a fallback.
- Accept at most 64 KiB; use the existing pinned russh key parser and RSA negotiation.
  Keep byte-buffer clearing and native cancellation/lifetime ownership in their layers.

## Rejected alternatives

- Enable the button without a native path: creates a nonfunctional promise.
- Put PEM bytes in host JSON or duplicate a key store: duplicates secret ownership.
- Retry a failed key as a password: changes the user's explicit authentication choice.
- Reimplement SSH key parsing/signing in Kotlin: duplicates the pinned protocol library.

## Consequences

Moving/revoking the selected document may require choosing it again. Its original
provider owns storage; the app does not claim to encrypt that original file.
Native credential buffers are temporary; managed/library internals are not claimed
to have universal erasure guarantees. Unused persisted grants are released on removal.

## Validation

Existing Android UI tests cover enabled key selection and the required-document state.
`SshIntegrationTest` creates ephemeral keys on the isolated OpenSSH fixture and checks
Ed25519, encrypted keys, RSA/PEM, wrong passphrases and no password fallback, with SFTP.
Device acceptance additionally uses the actual picker, saved host and reconnect path.

## Supersedes

None.

## Revisit when

The product adds managed key import, certificates, hardware keys or jump hosts.
