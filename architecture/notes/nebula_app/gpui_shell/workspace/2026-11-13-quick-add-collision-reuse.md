# Quick-add collision reuse and credential ordering

## Status

Accepted (#869 review, 2026-11-12).

## Context

The quick-add overlay only opens on a launcher search miss, but the final
validated destination is decided at save time: the user can start the form for
one address and edit it onto a destination that already has a saved profile.
`persist_quick_add` built a fresh subset profile (empty keys, default
connection options) and `SshProfiles::upsert` replaces the stored profile for
that exact destination, so saving silently discarded private keys, jump route,
label and icon and then connected with the degraded replacement. Password
storage also ran after `profiles.save()`, so a credential-storage failure was
reported by the form after the profile file had already changed.

## Evidence

- Save path: `nebula_app/src/gpui_shell/workspace/ssh_quick_add.rs`
  (`persist_quick_add_with`).
- Replace-on-exact-match semantics and the existence probe:
  `nebula_app/src/ssh_profiles.rs` (`SshProfiles::upsert`,
  `SshProfiles::contains_destination`).
- Review reproduction on PR #869 (2026-11-12): a populated
  `deploy@box.example` profile replaced through a quick-add save.

## Decision

- Save-time duplicate resolution against the final validated destination:
  when the host library already holds a profile for it, quick-add persists
  nothing and writes no credentials; it returns the destination and
  `add_ssh_terminal` connects through the saved profile. Editing keys, jump,
  label or icon remains the Settings editor's job via "More settings".
- Credential storage runs before the profile file is written: a
  credential-storage failure leaves the host library untouched and the overlay
  open with the error. The write set is one profile plus one credential, so
  all-or-nothing ordering is achievable here.

## Rejected alternatives

- Overwrite the existing profile with the subset form: destroys editor-owned
  fields and connects with the degraded replacement (the reported bug).
- Profile-save first with a partial-success message (the Settings editor's
  pattern for proxy credentials): the overlay has no UI state for "saved but
  password missing", so the kept-open form would invite a retry that now
  collides with the profile it just wrote.
- Auto-route the collision into the Settings editor: the overlay's purpose is
  connecting; silently switching pages breaks the one-flow intent recorded in
  `2026-11-03-launcher-add-and-connect-ssh.md`.

## Consequences

- Typing an existing host's address into quick-add connects with the saved
  configuration and changes nothing on disk or in the credential store.
- A stored password can exist for a destination whose profile save later
  fails; it is keyed by destination and overwritten or reused by the next
  successful save for that host.

## Validation

- `quick_add_collision_reuses_the_saved_profile_untouched`: a populated
  profile (keys, jump host, label, icon) survives a colliding save
  byte-for-byte and the injected credential recorder sees no call.
- `quick_add_password_failure_leaves_the_host_library_untouched`: a failing
  credential store is injected and the profile file is never created.
- `persist_quick_add_with` takes the credential write as a parameter,
  mirroring the closure-injected tests of `platform::credentials`, so tests
  never touch the user keychain.

## Supersedes

None. Amends the persistence paragraph of
`2026-11-03-launcher-add-and-connect-ssh.md`.

## Revisit when

The overlay gains field-level merge into an existing profile, or the host
library grows a create path outside the Settings editor.
