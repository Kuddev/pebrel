# Backup preference persistence and explicit archive operations

## Status

Accepted for the desktop settings integration.

## Context

Cloud storage configuration and archive operations share a settings page. Ordinary
configuration edits should survive leaving the page, while uploads, credential
writes and restoring data require their own explicit actions.

## Evidence

- `nebula_app/src/backup_remote.rs` already owns transport selection, credential
  access, archive names and retention. The settings view can reuse this adapter.
- `nebula_app/src/encrypted_backup.rs` owns encrypted archive contents and restore.
- The previous settings navigation hid the backup route despite an existing
  implementation. The desktop integration makes this existing capability visible.

## Decision

Persist the selected backup categories as an optional field in the existing
configuration. Older files retain `BackupSelection::default()` behavior.

`backup_remote/preferences.rs` owns one process-wide queue and at most one active
writer. New edits replace the pending configuration; an already running write
finishes before the next one. Accepted edits drain after their settings view
closes. Writes use a temporary file in the destination directory followed by atomic
replacement, and reject line breaks in single-line configuration values.

The view observes save revisions separately from network-listing revisions.
Changing storage invalidates old listings. Archive actions receive a captured
configuration, and native restore prompts are accepted before applying data.
Storage selection is blocked while an archive or credential operation is active,
including callbacks from an already-open provider menu.

Autosave never uploads an archive or writes a credential. Explicit credential
storage uses the existing platform adapter; the archive password remains in the
current view. Snapshot listing and selected restore reuse owned-name validation.

## Rejected alternatives

- Saving on the UI thread would put filesystem latency in input handling.
- One detached writer per edit could let an older write replace a newer value.
- Cancelling all accepted writes when the view closes would lose the last edit.
- A second backup backend in the UI would duplicate transport and retention rules.

## Consequences

The worker exists only while accepted edits are pending and adds no dependency or
daemon. Backups remain encrypted full archives with the existing ten-copy retention.
Automatic upload and incremental synchronization are outside this behavior.

## Validation

Preference tests cover coalesced edits, selected categories, view drop, rejected
configuration injection, owned archive listing and path traversal rejection.
GPUI tests exercise backup tabs, expanded categories, save feedback bounds and
controls at wide and narrow window sizes. These tests do not establish external
provider connectivity or desktop visual acceptance.

## Supersedes

None.

## Revisit when

Multiple processes edit the same backup configuration, or backup policy changes to
automatic upload, incremental archives or different credential ownership.
