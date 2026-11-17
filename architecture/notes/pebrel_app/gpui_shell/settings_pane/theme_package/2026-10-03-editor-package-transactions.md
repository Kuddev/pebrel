# Editor package transactions

## Status

Implemented and tested locally on Windows; dark/light native modal inspection
passed. Repository CI and other-platform visual acceptance remain pending.

## Context

Portable theme resources need an editor entry so sharing and installation do not
require a shell. Editing drafts and installing a package must not implicitly
activate runtime settings or discard an existing draft.

## Evidence

The package core already owns byte/path/integrity limits and staged publication.
The editor owns draft replacement, conflict prompts, wallpaper preparation, and
Save/Apply. Rendered tests reproduce a late picker result and a repeated click
while a small package transitions from committing to completed.

## Decision

Keep one pane-owned modal with export/import modes, bounded document/manifest
snapshots, input entities, and a sequence token. File selection and archive work
run asynchronously. A cancelled picker or inspection cannot update a reopened
modal. Committing work disables close controls and completes its atomic operation
if the window disappears; inspection cancellation discards its bounded result.

Inspection resolves a typed definition on the worker. Rendering performs no file
reads, JSON parsing, media decoding, hashing, or synchronous waits. Confirmation
reopens and validates the selected ZIP and compares both manifest and document
with the reviewed snapshot before installation. The installer remains the only
persistence and size authority; no archive bytes or decoded video frames are
retained in UI state.

Import adds a library theme without applying preferences. Editing an installed
copy uses the existing editor replacement confirmation and Save/Apply rules.
Export snapshots authorship, version, license, preview, and the current valid
draft. Prepared wallpaper ownership remains with the existing preview entity.

Cross-platform correctness, bounded memory, and responsive rendering are separate
acceptance requirements. Native tests cover the shared path on each selected
host. Buffer/pixel budgets describe owned resources, not universal process peaks;
local elapsed-time or working-set observations are not cross-machine guarantees.

## Rejected alternatives

- Apply a draft to obtain its package preview: violates the settings transaction.
- Install while merely inspecting a chosen file: makes cancellation publish data.
- Trust the earlier file inspection after confirmation: permits unseen replacement.
- Read or decode an archive from the renderer: blocks frames and duplicates budgets.
- Reuse a live completion action for an earlier painted Install button: repeated
  input can dismiss feedback instead of retaining the completed result.

## Consequences

The explicit modal actions preserve draft and runtime separation. Completed
feedback remains until an explicit close/edit action; backdrop input is consumed.
Package installation retains managed resources after library JSON deletion, as
recorded by the package core. Video/animation/shader execution remains unavailable;
a format check cannot stand in for playback, memory, or frame-scheduling acceptance.

## Validation

The Windows GPUI product build passed. All five rendered package regressions and
34 theme-studio cases passed, with one existing manual visual test skipped.
Cases cover background/preview export with identity, confirmation-only installation,
repeated clicks, changed/invalid archives, late pickers, visible hit targets, and
Tab/Escape in a narrow window. Internationalization contract tests passed (23, one
existing ignored); governance/name regressions passed (73). Native Nord and Warm
Sand modal screenshots confirmed layout, labels and theme roles. This is local
Windows evidence; other-platform visual and repository CI acceptance remain separate.

## Supersedes

None.

## Revisit when

Community downloads, reference-aware resource cleanup, or a supported media
pipeline requires a different transaction or frame/resource lifetime.
