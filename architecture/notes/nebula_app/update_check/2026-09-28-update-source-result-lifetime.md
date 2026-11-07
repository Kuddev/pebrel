# Update source result lifetime

## Status

Proposed with PR #294; maintainer review pending.

## Context

The configurable release source is read by manual checks, delayed startup checks,
and cached download hydration. A request can finish after the source changes or
Reset All restores the official source. Per-settings-pane sequencing protects
only one manual check and cannot invalidate startup work or results already queued
for another window.

## Evidence

- update_check::spawn_once and update_check::spawn_gpui_once publish background results.
- SettingsPane::check_for_updates can open its prompt after its state update.
- update_download::cancel invalidates the existing download generation, and
  cached assets are validated against the configured source before reuse.
- The focused updater tests run through the repository's GitHub Actions workflows.

## Decision

Increment one process-local generation when the release source changes or all
settings reset. Each manual or startup check captures its generation and drops a
result when it is no longer current. GPUI update events carry the generation until
the main event dispatcher applies them. The About page also keeps its existing
per-pane sequence for repeated checks. Source invalidation clears the pane's
displayed check result and cancels the current download session.

## Rejected alternatives

Comparing source URLs alone would accept an old result if the setting changed
away and back while that request was running. Per-pane sequencing alone leaves
startup checks and another window's event queue unprotected. Cancelling every
network request would require new ownership across the synchronous release
fetcher; ignoring stale results uses the existing generation-based pattern.

## Consequences

Changing or resetting the source invalidates old results across all windows.
A check for the current generation remains usable. A stale cached asset does not
become installable because the existing asset validation still checks its source.

## Validation

The regression is
update_check::tests::invalidating_the_release_source_rejects_an_in_flight_result.
The PR description records GitHub Actions status for the final head; this note
does not claim UI end-to-end validation.

## Supersedes

None. This records result lifetime for
[the configurable release source](2026-09-25-custom-release-source.md).

## Revisit when

Revisit if release checks become cancellable and are actually cancelled on source
change, or if the updater moves source-generation ownership to another module.
