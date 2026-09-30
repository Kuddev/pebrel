# Installed release descriptions

## Status

Proposed in this PR.

## Context

Users need to read the installed version's GitHub Release description in Settings,
and see it once after an in-app update succeeds. A newer release can appear before
that first launch; checking `/latest` cannot identify the installed description.

## Evidence

`update_check.rs` already parses the Release body for checksums but discards it.
`update_download/handoff.rs` records the installer transaction, executable and
result and verifies the running version when recovering the workspace.
`update_state.json` already serializes prompt preferences with process/file locks.
The GPUI component `TextView` already renders selectable Markdown in the product.

## Decision

Retain the Release body as separate updater metadata. Cache it during a successful
update check, then snapshot the matching version in the existing handoff directory
before authorizing installation. This adds no installation authority or helper
protocol field. Missing cached notes do not prevent installation.

On startup require a successful result matching the transaction, installation and
running version. Load the snapshot, or fetch that exact version's tag when no
snapshot exists. Failed/offline reads leave the notice pending for a later launch.
Claim the notice with the existing prompt-state process/file locks before showing
it in the selected window. The optional `release_notes_shown` field defaults to
unset for existing state files and is independent of update snooze/skip settings.

Settings and the dialog share a read-only Markdown view with loading, empty,
failure/retry and external-release states. Remote descriptions are rendered as
content; only HTTP(S) links are opened by the view. Settings remain available on
externally managed distributions; their updates do not create in-app handoffs.

## Rejected alternatives

- Inferring success from a changed version: also triggers on fresh installs,
  manual installs and failed updates followed by a rollback.
- Showing the latest description: can describe a version not yet installed.
- Using the workspace restore acknowledgement: restoration and reading notes have
  independent lifecycles; an offline metadata fetch must not consume the notice.
- A second rich-text renderer or dependency: the existing TextView is sufficient.

## Consequences

Release text is stored under the existing updater directory, outside user
settings. No new thread is added; discovery uses the existing startup check and
view-owned background tasks. Automatic installation remains Windows/macOS only;
other distributions can read their installed version's notes in Settings.

The shown marker gives at-most-once presentation across normal restarts and
concurrent processes. As with existing update prompts, a crash after persisting
that marker but before painting can consume the notice; Settings still exposes it.

## Validation

Focused regressions cover exact-version text, persisted snapshots, successful
handoff, duplicate claims, rollback exclusion and existing workspace recovery.
PR evidence records compilation, rendered UI checks and native CI separately.

## Supersedes

None.

## Revisit when

The updater accepts another release source, the handoff protocol changes, or
release notes need a user acknowledgement rather than once-only presentation.
