# Independent identities for saved SSH host copies

## Status
Implemented in the working tree. Focused tests, a GPUI product build and native
copy/persistence/edit acceptance passed; a separate menu teardown failure remains.

## Context
The host menu's Copy action opened a draft editor and cleared its address.
The requested behavior is to immediately save another host with the same connection
settings. Profiles, list entries, proxy secrets and passwords previously all used
the connection destination as their key, so saving the same destination again
would replace the original host.

## Evidence
`SshProfiles::upsert` replaces a profile with the same destination.
`SshHostLists::merged` deduplicates entries by that identity.
`SshDestination::original` also scopes credentials and connection-pool routes.

## Decision
Keep existing identities unchanged. New copies receive a random application-owned
identity and an optional `targets` entry in the same profile document. Its value
is the connection address or OpenSSH alias, never another copy's identity.
The native SSH resolver expands the connection target while retaining the copy's
identity for authentication and session ownership. The editor and host search
use the actual target. Editing or deleting a copy does not mutate its source.
The system-SSH wrapper likewise forwards the real address (normalizing explicit
ports for OpenSSH), while its AskPass environment retains the copy's identity.

Copy authentication settings, key references, proxy/jump settings and organization.
Read and write secrets only through the existing system credential adapter, using
new credential keys. Save profile metadata after credential writes succeed; remove
newly written credentials if saving fails. The existing profile lock and stale
snapshot check remain authoritative. Perform this work on the background executor,
and show success only after persistence completes. Preserve distinct targets in
credential-free CSV exchange through an optional `connect_to` column.

## Rejected alternatives
Saving another profile under the original address overwrites it. Opening an editor
or clearing the address fails the requested interaction. Writing generated aliases
into the user's OpenSSH configuration would modify a separate source of truth.

## Consequences
Existing profiles require no migration. Versions that do not understand `targets`
cannot connect the new copies; downgrading is not a supported copy transport.
OpenSSH aliases retain their existing dependence on the user's SSH configuration.

## Validation
43 focused tests passed, including independent metadata/targets, copies of copies,
CSV roundtrips, credential rollback, native destination resolution, system-SSH
argument forwarding, hidden settings navigation/search and keyboard menu failure.
The GPUI product build passed after regenerable Cargo outputs were cleaned.

Native Windows mouse and keyboard actions created copies immediately without an
editor. Two successive mouse copies preserved the original profile and targets.
A fresh product process loaded both copies; editing one copy's address retained
its identity and left the source and sibling unchanged. The built executable also
expanded both copied endpoints correctly through the real OpenSSH `-G` command.
This does not claim a new remote authentication or SFTP acceptance run.

The ignored native preview test fails its exit-time PopupMenu leak assertion.
Opening and dismissing a right-click menu without invoking Copy or writing any
profile reproduced the failure. The pinned gpui-component ContextMenu subscription
captures its own shared state strongly; changing preview shutdown ordering did
not resolve the failure. Leak detection remains enabled. This separate dependency
lifecycle issue is not treated as a failed host copy or a passing native test.
The workspace architecture gate still reports its existing workspace.rs line
budget violation; no budget or unrelated source was changed to hide it.

## Supersedes
None.

## Revisit when
The shared host model adopts explicit identities for all saved profiles.
