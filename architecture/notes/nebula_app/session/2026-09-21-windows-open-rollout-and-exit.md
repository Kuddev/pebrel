# Windows open-rollout identity and bounded shutdown

## Status

Implemented; native Windows adapter tests passed. Product and CI validation are
recorded with the integrating change.

## Context

The same persisted Codex thread ID can resume the same conversation across CLI
restarts. The defect is losing or overwriting that ID in the host application's
snapshot, not a requirement to select a different conversation each time.
Windows previously depended on Hook facts while Linux/WSL could inspect the
Codex process's open rollout. Waiting indefinitely for missing identity also
made closing the window impossible when no usable Hook arrived.

## Evidence

- [The shared probe](../../../../nebula_app/src/platform/ai_session_identity.rs)
  verifies rollout filename and first-record metadata, rejects explicit
  subagents and refuses multiple main conversations.
- [The Windows adapter](../../../../nebula_app/src/platform/ai_session_identity/windows.rs)
  reads per-process handle snapshots for the pane shell's verified descendants.
- [Command completion](../../../../nebula_app/src/gpui_shell/terminal/view/runtime.rs)
  has a native CMD route with no exit status; missing-ID recovery previously
  required a nonzero status.
- [Window close](../../../../nebula_app/src/gpui_shell/workspace/closing.rs) and
  [application quit](../../../../nebula_app/src/gpui_shell/workspace/windowing/shutdown.rs)
  previously aborted after identity discovery timed out.

## Decision

Keep the existing provider-owned transcript model and optional snapshot fields.
On Windows, anchor discovery to the live pane shell PID and creation time, then
reuse the shared process-tree traversal and PID-reuse checks. Inspect only Codex
processes in that tree. Recheck each process creation time after opening it.
Duplicate candidate disk handles only to obtain their path and file identity;
open an independent file object to read the bounded first metadata record and
verify it still identifies the same file. Never seek a duplicated live handle.
The shared metadata parser remains the identity authority on every platform.

Run potentially blocking Windows file APIs in a short-lived invocation of the
same executable, before GUI or hook initialization. The owning background task
bounds output and kills/reaps the helper after two seconds. Per-process handle
snapshots have a 2 MiB cap. No persistent service or dependency is added. Only
identity metadata crosses the helper boundary; conversation bodies do not.

Save the ID and native path together. A lifecycle-only Hook cannot overwrite an
identity established from the open file. Apply results only to the same command
epoch with an unchanged target; failed probes preserve the last known target.

A completed CMD restore may use its exact missing-ID diagnostic despite lacking
an exit code. Also expose an explicit conversation choice after any failed
Codex restore, independent of diagnostic language. Preserve the old target until
the chosen conversation is acknowledged; never automatically retry a chooser.

After bounded identity discovery, closing offers cancellation or saving available
state and exiting. Consent covers missing identities, not save errors or changed
editor drafts. Save durable state before stopping terminals. The same choice
applies to whole-application exit and update exit.

## Rejected alternatives

- Match by cwd, newest file or directory scan: neighboring panes may have distinct
  conversations in the same directory.
- Read/seek a duplicated file handle: that changes the live process file cursor.
- Query handles on the UI thread or abandon a stuck worker thread: neither
  bounds the lifetime of a blocking native request.
- Block exit forever when no ID arrives: the application cannot guarantee that
  every external integration will report metadata.
- Treat a missing exit code as success or require English diagnostics for every
  recovery action: native CMD and localized providers need usable fallback paths.

## Consequences

Native discovery requires access to the same user's process/file handles. When
access, timing or identity checks fail, the saved target remains intact. A
snapshot that never had a valid ID still needs an explicit choice. Closing with
missing IDs preserves available state but cannot promise automatic resumption
for those unidentified conversations.

## Validation

Owned CMD and PowerShell processes keep real rollout fixtures open. Native tests
verify pane/process separation, stable ID and path, unchanged live file cursor,
filename/metadata mismatch, multiple main rollouts, process creation mismatch,
metadata-only output and helper timeout. UI regressions cover the CMD completion
route, explicit selection without a recognized diagnostic, retained native IDs
and the cancel/continue exit prompt. These fixtures do not make provider model
calls or claim success for every external CLI.

## Supersedes

Extends [native resume identity](2026-09-21-native-resume-identity.md) for Windows
process discovery and missing-identity shutdown; its provider identity contract
remains in force.

## Revisit when

Codex changes its open-file or metadata contract, Windows removes the handle
query, or the application gains persistent terminal processes.
