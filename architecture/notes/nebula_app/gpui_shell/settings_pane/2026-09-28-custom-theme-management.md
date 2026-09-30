# Stable custom-theme editing and failure-safe deletion

## Status

Implemented on 2026-09-28.

## Context

The original Theme Studio workflow always saved an editor draft as a new
library document. Repeated refinement therefore produced duplicate themes and
gave a saved custom theme no edit, rename, or delete lifecycle in the picker.

The library document and the active-theme preference are separate durable
resources with independent revision checks. Deleting an active document before
publishing its built-in fallback can leave settings pointing to a missing file
when another window wins the preference revision race.

## Evidence

- [Theme library storage](../../../../../nebula_app/src/theme_library/store.rs)
  gives each custom document a stable id and monotonically increasing revision.
- [Theme preferences](../../../../../nebula_app/src/theme_library/preferences.rs)
  conditionally replace the settings file using the bytes observed when the
  picker or editor opened.
- [Theme deletion](../../../../../nebula_app/src/gpui_shell/settings_pane/appearance_picker/theme_delete.rs)
  coordinates those two revision domains on a background executor.
- [Rendered Theme Studio tests](../../../../../nebula_app/src/gpui_shell/settings_pane/theme_studio_tests.rs)
  exercise the picker, confirmation dialog, persisted documents, runtime
  settings, and stale revision outcomes through real controls.

## Decision

- Creating from a built-in template still writes a new independent library
  snapshot. Editing a saved custom theme retains its id and uses its loaded
  document revision as the replacement precondition. Renaming is part of that
  same replacement rather than an identity change.
- Save and Apply closes the settings workflow after persistence so the terminal
  shows the result. Reopening the same custom theme resumes from the stored
  document instead of creating another copy.
- Deletion remains explicit and confirmed. An inactive custom theme needs only
  its document revision check.
- Active deletion publishes the built-in fallback with the loaded preference
  revision before removing the library document. A stale settings revision
  therefore leaves the active source untouched. If a newer library revision
  appears after the fallback is published, that newer document remains and the
  picker reports the conflict while aligning itself with the persisted fallback.
- Picker and editor completion handlers retain their session sequence checks.
  An old background result cannot mutate a reopened or destroyed workflow.

## Rejected alternatives

- Deleting the active file first can destroy the only source before a settings
  conflict is known.
- Deleting first and recreating the document after a preference failure adds a
  second race: restoration can itself lose to a new document or I/O failure.
- Saving every edit under a new id preserves the old workflow but recreates the
  duplicate accumulation that this interaction is intended to remove.
- Ignoring revisions or silently overwriting either file would discard changes
  made by another window.

## Consequences

An active delete is intentionally safety-biased. A settings conflict performs no
library deletion. A later library conflict can deactivate the theme without
deleting the competing document; the visible error and retained document let the
user inspect it and retry.

The library and preference files are not claimed to be one crash-atomic
transaction. The ordering guarantees that a recoverable document is preferred
over a dangling active preference.

## Validation

Rendered regressions cover in-place edit and rename, repeated reopen, confirmed
active deletion, stale preference preservation, and a newer library document
surviving a stale delete. The i18n contract covers the CRUD message identifiers
and placeholder shapes in all supported catalogs.

The focused `active_custom_theme` GPUI run passed all three deletion tests. The
application and isolated i18n contract runs each passed 23 tests with the manual
microbenchmark ignored. `scripts/check_architecture.py --base origin/main` and
targeted Rust formatting passed. The required native CI matrix remains the merge
authority.

## Supersedes

[ADR-0011](../../../../../docs/architecture-decisions.md#adr-0011--editable-theme-snapshots-and-preview-before-application)
only for its statement that saving an editor draft always writes an independent
copy. Its ownership, format, preview, rendering, and bounded-I/O decisions remain
in force.

## Revisit when

Theme documents and active selection move behind one durable transaction or an
operation journal; or product requirements prefer automatic conflict merging to
the current explicit retry behavior.
