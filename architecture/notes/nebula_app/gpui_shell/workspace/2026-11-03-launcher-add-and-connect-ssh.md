# Launcher "add and connect" fallback row

## Status

Accepted (#864).

## Context

Cmd/Ctrl+K and the command palette share one row projection
(`workspace/palette.rs::filtered_palette_rows`). Both list the same SSH host
authority (`SshHostLists`: saved/pinned plus `~/.ssh/config`). Typing a host
that is not in that authority produced an empty result with no next step: the
user had to open Settings → SSH, add the host, then come back and connect.

The rows are a projection, not a connection authority. A new host therefore
cannot be connected straight from the row: it has no profile, no credentials
and no label. The editor already owns validation, persistence, credential
storage and the `SettingsPaneEvent::LaunchSsh` transition that the host
library's "Connect" button uses.

## Evidence

- Row projection and filtering: `nebula_app/src/gpui_shell/workspace/palette.rs`.
- Existing edit dispatch pattern (`window.defer` into the settings pane):
  `nebula_app/src/gpui_shell/workspace/launcher_menu.rs`.
- Prefilled editor and save pipeline:
  `nebula_app/src/gpui_shell/ssh_settings.rs` (`open_ssh_editor`,
  `save_ssh_editor`).
- Connect transition: `SettingsPaneEvent::LaunchSsh` handled in
  `nebula_app/src/gpui_shell/workspace/settings_navigation.rs`.

## Decision

- When filtering yields no rows and the query looks like a host address
  (non-empty, no whitespace, at least three characters, contains `@` or `.`),
  append one row whose action is `WorkspacePaletteAction::AddSshHostAndConnect`.
- Address validity is decided by the editor's save-time validation, not by the
  row predicate. A false positive only costs an opened editor.
- The action opens a workspace-owned overlay (`workspace/ssh_quick_add.rs`)
  instead of navigating to Settings: the user's intent is "connect", and a
  page switch in between is not part of that intent. The overlay renders above
  the terminal, closes on Esc/Backdrop, and Enter submits.
- Field order and wording mirror the Settings editor's basic form exactly
  (name optional, then address+port on one row with the address hint and a
  composed-destination preview, then username, divider, the same segmented
  authentication row in the same mode order). Divergence on purpose: the
  overlay has no group/tags/notes organization block and no private-key file
  picker. Choosing "Key" shows a hint and saving is rejected with guidance,
  because a public-key profile without keys cannot connect. Validation,
  destination composition, persistence and credential storage reuse the
  existing authorities (`validate_ssh_destination`, `join_destination_*`,
  `SshProfiles::save`, `ssh_credentials::store_password`); the full editor in
  Settings remains the authority for proxy/jump/key/icon configuration, and
  both write the same `ssh_profiles.json`.
- "More settings" carries the composed destination into the Settings editor
  (`manage_launcher_ssh(dest, edit=true)`), so half-filled work continues
  there instead of being retyped.
- Persistence is split from connecting (`persist_quick_add`), so failures keep
  the overlay open with the reason, and tests can verify the disk side without
  opening a session.

## Rejected alternatives

- Connect the typed address directly without saving: the existing design
  records a first connection in `saved_hosts` anyway, and password hosts still
  need the editor, so it would split one flow into two authorities.
- Only add the row to the launcher and not the command palette: both render the
  same projection, so gating one would fork the row source.
- Treat the row as "edit this host": `Some(destination)` skips the new-host
  username default, which produced `core.example.cn` instead of
  `root@core.example.cn` in testing.
- Persist the intent: it describes one in-flight editor session, not user
  configuration.

## Consequences

- New-host onboarding from the launcher is one flow: search → add → save →
  connected tab, using the same profile/credential/label path as the host
  library.
- Quick Jump folder/agent scopes and the launcher's Shell filter never show the
  row; All and SSH views do.
- A host typed but never saved leaves no trace; cancel and Esc clear the intent.

## Validation

- `workspace::ssh_quick_add::tests::quick_add_draft_composes_and_validates`
  covers destination composition, the port range and authority rejection.
- `workspace::ssh_quick_add::tests::quick_add_persists_a_nameless_profile_before_any_connection`
  covers the disk side against a temp file: no auto "Host N" label, Auto auth.
  The public-key save guard is asserted by the same module's validation test
  path (`KeysNeedAdvancedEditor`).
- `workspace::palette::palette_tests::plausible_ssh_query_needs_host_shape`
  covers the row predicate.
- i18n contract (`HostsAddAndConnect` with a `{destination}` placeholder),
  `cargo fmt`, and `scripts/check_architecture.py --base origin/main` pass.

## Supersedes

None.

## Revisit when

The host library gains a way to create a profile without the editor (for
example a dedicated quick-add dialog), or the launcher starts owning connection
state rather than projecting it.
