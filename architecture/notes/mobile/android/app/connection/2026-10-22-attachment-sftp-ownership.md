# Attachment SFTP ownership

## Status
Accepted for implementation.

## Context
Uploading a document from an authenticated terminal opened a second SSH login.
Without a saved password the UI required a password even for a key-only host.

## Evidence
`TerminalAttachmentFlow` previously requested credentials and `TerminalAttachments`
opened an independent exec connection. `LocalSession.files` already owns an SFTP
client borrowing the authenticated terminal transport.

## Decision
Borrow that SFTP client. Stage the bounded SAF input locally, upload through the
existing atomic SFTP transfer, and insert the resulting quoted path into the draft.
No input is sent to the shell. Selection, cancellation and completion belong to a
specific terminal instance, not just its stable tab ID.

## Rejected alternatives
Remembering another password or opening an exec connection would repeat host
verification and authentication unnecessarily and diverge from file browser behavior.

## Consequences
The server must provide SFTP; POSIX shell tools are no longer required for uploads.
Cancellation releases the upload handle and staging files without closing SSH.
Native uploads retain their existing exclusive creation and mode 0600 semantics.

## Validation
`TerminalAttachmentsTest` covers upload byte integrity, transfer failure and
cancellation with a still-usable borrowed SFTP client. Device SSH integration remains
necessary for a packaged release.

## Supersedes
None.

## Revisit when
The server does not offer SFTP and an explicitly selected fallback is required.
