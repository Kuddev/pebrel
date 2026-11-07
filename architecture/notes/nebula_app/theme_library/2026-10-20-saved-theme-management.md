# Saved custom theme editing and deletion

## Status

Implemented with native regression coverage.

## Context

The theme picker offered custom themes only as templates: the first editor save
forked them. The store already supported saved revisions and deletion, but users
had no explicit way to update or remove a saved theme. An active theme reference
and its library document live in separate files.

## Evidence

- [Editor save ownership](../../../../nebula_app/src/gpui_shell/settings_pane/theme_editor.rs)
  distinguishes template copies from updates guarded by the original document.
- [Theme storage](../../../../nebula_app/src/theme_library/store.rs) checks the
  full document under library and target locks before deletion.
- [Preference publication](../../../../nebula_app/src/theme_library/preferences.rs)
  uses the existing settings lock, byte revision and atomic replacement.

## Decision

Offer Edit and Delete beside the selected custom theme preview. Explicit Edit
retains its identity and source snapshot; Customize and template selection retain
their copy semantics. Save only updates the saved document while leaving the
current runtime snapshot alone; Save and apply also publishes the selection.

Delete requires confirmation and checks both the theme snapshot and preference
revision. Hold the settings lock while checking whether the selected library ID
is active. For an active ID, persist the normal default-theme selection before
removing the document, so a successful removal cannot leave an active file
reference dangling. Use the existing store locks for the final snapshot check.

A failed preference write leaves the document intact. If deletion fails after
the default was saved, retain the document, refresh the runtime, report the
partial result and keep the new preference revision available for retry.
Do not remove package resource directories as a side effect of document deletion.

Run file operations on the background executor. The picker owns busy state and
a session generation; an old completion cannot modify a replacement picker.

## Rejected alternatives

- Treating Customize as in-place editing breaks existing template-copy behavior.
- Comparing only numeric revisions can discard external same-revision edits.
- Deleting first can strand the persisted active reference if fallback publication fails.
- Silently rolling back a published fallback can overwrite a later settings change.

## Consequences

The current runtime can differ from a saved edited theme until Apply or reload.
Delete failures can leave the default applied while the original document remains;
the UI reports this state explicitly. Non-cooperating external writers retain the
existing race limits documented by the preference and store adapters.

## Validation

Theme-library tests cover active/inactive deletion, stale preference and document
snapshots, settings locks, invalid updates, deletion failure and retry. Native
control tests cover repeated in-place editing, keyboard access, cancellation,
index refresh, active-theme fallback, retry and a narrow picker. The i18n contract
checks catalog generation, placeholders, fallback and allocation behavior.

## Supersedes

None.

## Revisit when

Theme identity, persistence transactions or package resource ownership changes.
