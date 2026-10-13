# Completion requests without terminal-state ownership

## Status

Implemented; acceptance evidence is recorded with the associated change.

## Context

The first semantic source introduced source-specific request construction in
the GPUI input adapter. Background calculation also constructed an entire
`NebulaPaneState`, although it only needed input, cwd and execution environment.
Adding more sources there would make UI code own source admission rules and let
candidate computation accidentally depend on unrelated terminal state.

## Evidence

- `completion::Session::request` now freezes the input and admits the local Git
  execution context only for eligible requests; ordinary input avoids that clone.
- `display::suggest_engine::{Input, Candidates, calculate}` carries the data used
  by history, directory, command and filesystem matching without pane access.
- `gpui_shell/terminal/view/completion.rs` still owns input capture, task spawning,
  stale-result checks and UI application. These require a concrete view lifetime.
- `gpui_shell/terminal/suggest.rs::Pending` cancels the shared request token when
  the view releases its task. The token itself does not depend on GPUI.

## Decision

Keep one application entry in `completion.rs`. It owns the process-shared history,
directory and command handles and a small per-pane session containing source
caches. Requests own only values needed by the worker. Calculation returns a
fresh candidate value; it cannot mutate command execution, grid or popup selection.

Reuse the existing sources, task executor, bounded process adapter and cache
invalidation. The local Git source remains in `git_completion.rs`; semantic parsing
and edits remain in `nebula-completions`. Remote directory requests continue to
use the existing WSL/SFTP adapters. Host filesystem data cannot fill remote inputs.

Keep the existing `display::suggest_engine` compatibility entry for the optional
legacy renderer and its callers. It adapts pane state to the same input/result
calculation. Its physical directory is not proof of a renderer dependency. Do not
rename every shared type or migrate all terminal state as part of this boundary.

## Rejected alternatives

- Leave source selection in the GPUI worker: each new source would add source
  knowledge to input and view lifecycle code.
- Clone or construct a pane state for each calculation: unrelated mutable state
  would remain part of the worker interface and its initialization cost.
- Add a generic provider registry, JS runtime or executor: current sources need
  explicit selection, not runtime plugins or another task lifecycle.
- Move popup selection into the computation core: keyboard focus, dismissal and
  viewport ownership follow the view, not a background request.
- Relocate all legacy files first: the shared calculation can be isolated without
  a broad compatibility migration.

## Consequences

New sources can be admitted at the application boundary without teaching the
view their cache or shell rules. Windows, macOS and Linux use this same path;
native process and shell differences remain in existing adapters. No dependency,
feature flag, persistent format, executor or polling budget changes.

This is a narrower request boundary, not a fully independent application crate.
Shared presentation types remain exposed through the feature-selected `display`
facade, and terminal-owned UI state still resides in `NebulaPaneState`.

## Validation

The application request tests run real Git discovery, cache invalidation,
cancellation and filesystem environment isolation without creating a view.
Existing completion tests cover legacy state adaptation, history lock scope and
remote transitions. Existing GPUI and native-shell acceptance exercise the public
input path and all three completion modes after extraction. Native desktop
acceptance must be distinguished from platform compilation and unit tests.

## Supersedes

Supersedes the placement of shared sources and semantic dispatch in
`../git_completion/2026-09-30-local-branch-context.md`; that decision's source scope,
process bounds, cancellation and cache semantics remain applicable.

## Revisit when

A second semantic source needs shared admission logic, a remote source gets a
verified execution channel, or another frontend needs the same request API.
