# SSH recovery belongs to the open tab

## Status
Accepted for implementation.

## Context
Retry previously closed the old terminal and navigated through the login form.
Removing the selected ID caused navigation to fall back to the home page and
lost the tab draft. Direct SSH had no network/resume recovery owner.

## Evidence
`MainActivity.connectHost` previously called `closeTerminal` before `ssh`.
`SessionRepository` terminal callbacks updated entries by ID without checking
which transport generation produced the callback.

## Decision
Keep the tab ID, draft and remote attachment when replacing a failed transport.
Guard callbacks and borrowed SFTP access by connection/terminal instance.
Retry only previously established connections on transient network errors,
with one bounded-backoff job per tab. Keep the foreground session service alive
while a tab is reconnecting; closing the tab cancels recovery.

Reuse `HostCredentialStore` encryption for session-only ciphertext in memory.
Unsaved passwords/passphrases are not persisted, and plaintext arrays are erased
after authentication/encryption. Closing a tab discards its encrypted record.
An actual authentication failure offers a credential prompt within the terminal.
Host pin or authentication failures do not enter an automatic retry loop.

Keep discovered remote multiplexer session, window and pane IDs rather than a mutable title.
Reattachment validates the server/session identity. When discovery is unavailable,
a configured remote multiplexer retry attaches by the original configured name without creating
a replacement. Explicitly new sessions retain their normal startup behavior.

## Rejected alternatives
Reopening the host form loses navigation and requires a second action. Keeping
plaintext passwords or silently persisting unsaved credentials is unnecessary.
Reusing a tab ID without callback guards lets a late disconnect affect a new socket.

## Consequences
Activity/view detachment only pauses rendering. Retry continues while Android
allows the foreground service to run and resumes after route changes. Process
termination still destroys in-memory sessions; this is not process-death recovery.
A plain SSH shell cannot restore a process killed by its server after disconnect.

## Validation
`SshRecoveryTest` drives real repository transitions with a shadow native terminal:
identity/draft/attachment retention, stale callbacks, unrelated tab survival,
background route recovery and close cancellation. `SshReconnectTest` covers
backoff, error policy and non-creating remote multiplexer reattachment commands.
Kotlin compilation and the focused JVM tests passed; device acceptance is separate.

## Supersedes
None. Desktop relay recovery retains its existing ownership.

## Revisit when
Implementing process-death restoration or a roaming transport protocol.
