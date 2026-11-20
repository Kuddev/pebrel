# Saved commands in encrypted command backups

## Status
Proposed for Issue #433.

## Context
The command manager persists commands and group metadata in saved_commands.json.
The encrypted command category previously collected only shell history files.

## Evidence
saved_commands::store_path owns the omitted file. encrypted_backup::collect_from
and validate_archive shared the history-only allowlist across local and remote
backup destinations. Both the current archive and command store use version 1.

## Decision
Include the exact saved_commands.json file in the existing opt-in CommandHistory
category. Label the category as commands and history. Reuse the command store's
parser and validation before restoring. Preserve the stored bytes, encryption,
atomic writes, selective restore and encrypted recovery-point semantics. A restore
or undo notification reloads an open command manager, including its group selection.

## Rejected alternatives
A separate category would introduce a new persisted selection field for the same
user-selected command scope. Directory walking would broaden the security boundary.
Duplicating the command schema would allow import and command management to drift.

## Consequences
Commands and their organization travel with the selected command category. Old v1
archives without the file leave the local store untouched. Existing old application
versions may reject new backups containing the newly allowed entry; the wire format
is unchanged and no forward-reader compatibility is promised. No command is run
by restore; the command manager's existing load and explicit execution paths remain.

## Validation
Regression checks cover collection, encrypted round trip, selection, group metadata,
undo, old v1 packages and rejection of malformed/future command-store versions before
writes. All checks execute through GitHub Actions.

## Supersedes
None.

## Revisit when
The command store gains a new format or backup category boundaries change.
