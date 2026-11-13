# Quick-add collision reuse and credential ordering

## Status

Accepted (#869 review, 2026-11-12). Amended 2026-11-13: the credential
ordering bullet was superseded by the leased save transaction below (#869
review round 2).

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
- Review reproduction on PR #869 (2026-11-13): with the plain
  "credential first" ordering, two windows quick-adding the same destination
  interleave as A loads the library, B saves profile and password, A writes
  its password, then A's `profiles.save()` fails on the changed file — the
  library keeps B's profile while A has already overwritten B's credential.
- Review reproduction on PR #869 (2026-11-13, second round): with the
  rollback keyed on `prior` alone, a save that never writes a password
  (auto auth, or password auth with the field left empty) shared the
  `prior = None` shape with "wrote, nothing existed" — a failing commit
  then called the delete path on a credential this transaction never
  touched. Reproduced on Linux with a pre-seeded credential, an existing
  lock file and a read-only directory: both no-write saves errored on the
  commit and deleted the pre-existing password.

## Decision

- Save-time duplicate resolution against the final validated destination:
  when the host library already holds a profile for it, quick-add persists
  nothing and writes no credentials; it returns the destination and
  `add_ssh_terminal` connects through the saved profile. Editing keys, jump,
  label or icon remains the Settings editor's job via "More settings".
- Duplicate check, credential write and library commit run as one save
  transaction under a single `atomic_file` lifetime lease
  (`SshProfiles::save_leased`): another window's save cannot slip between
  the check and the commit, and the lease-busy error tells it to retry.
  Within the transaction the credential write still precedes the commit, so
  a credential-storage failure leaves the host library untouched. When the
  commit itself fails (a lease-bypassing writer changed the file, or the
  disk errored), only a password this transaction actually wrote is rolled
  back — the prior password is restored, or the just-written one is deleted;
  a save that wrote no password leaves an existing credential untouched.

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
- Commit first with a file rollback on credential failure: restoring the
  pre-save bytes also works while the lease is held, but the interleaving
  and commit-failure regressions the review asked for then have no seam to
  pin them (everything injectable sits after the commit), and the write set
  hits the profile file twice per save.

## Consequences

- Typing an existing host's address into quick-add connects with the saved
  configuration and changes nothing on disk or in the credential store.
- The window-vs-window interleaving cannot produce a mismatched pair
  anymore: a concurrent quick-add either reuses the freshly saved profile
  (collision path) or is rejected as lease-busy and retried on a fresh
  snapshot.
- A failed library commit can still leave a lease-bypassing writer's
  profile in place, but never paired with this transaction's password: the
  credential is restored or removed before the error is reported.

## Validation

- `quick_add_collision_reuses_the_saved_profile_untouched`: a populated
  profile (keys, jump host, label, icon) survives a colliding save
  byte-for-byte and the injected credential recorder sees no call.
- `quick_add_password_failure_leaves_the_host_library_untouched`: a failing
  credential store is injected and the profile file is never created.
- `quick_add_transaction_rejects_a_window_save_in_the_middle`: window B's
  API save fired from inside the transaction is rejected with the lease-busy
  error and A's profile and password are the only survivors.
- `quick_add_commit_failure_rolls_the_password_back`: an out-of-band writer
  makes the commit fail; with no prior password the typed one is deleted,
  with a prior one it is restored.
- `quick_add_commit_failure_without_a_password_write_keeps_the_existing_credential`:
  auto auth and empty-password saves with a pre-existing credential fail the
  commit through a read-only directory (existing lock file keeps the lease
  working) and record zero credential operations — the pre-existing password
  survives.
- `persist_quick_add_with` takes the credential operations as a parameter
  (`CredentialOp`), mirroring the closure-injected tests of
  `platform::credentials`, so tests never touch the user keychain.

## Supersedes

None. Amends the persistence paragraph of
`2026-11-03-launcher-add-and-connect-ssh.md`.

## Revisit when

The overlay gains field-level merge into an existing profile, or the host
library grows a create path outside the Settings editor.
