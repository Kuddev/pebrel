# One explicit source for interactive split launches

## Status
Proposed for maintainer review in PR #484.

## Context
Splitting beside WSL or SSH needs focused inheritance and a deliberate local-shell choice.

## Evidence
[Issue #372](https://github.com/Kuddev/pebrel/issues/372) describes focused inheritance;
[PR #351](https://github.com/Kuddev/pebrel/pull/351) supplied the adapted WSL rules.
[The three-source proposal](https://github.com/Kuddev/pebrel/pull/484#issuecomment-6003205050)
and [owner review](https://github.com/Kuddev/pebrel/pull/484#issuecomment-6003895446) establish the choices and factory default.

## Decision
Persist `split_shell_source=default|focused|ask`; missing or invalid values use Default.
Declaration, value and display order are Default → Focused → Ask.
Default resolves the configured Shell/Profile and inherits only a host-visible cwd;
without one, a profile retains its startup cwd. Focused copies the pane's frozen
Shell/Profile/SSH identity and live cwd using the existing guest/user rules.
Ask reuses the launcher, capturing pane id and direction; cancel creates nothing,
and a closed source is never replaced by the new focus. Settings apply immediately
to subsequent interactive requests. Runtime API splits remain immediate Focused.

## Rejected alternatives
An implicit boolean cannot express three choices; defaulting to Focused changes the
unspecified-preference behavior. Tab metadata cannot identify a mixed-shell pane.
Retargeting after source closure or waiting for a picker in the API breaks caller intent.

## Consequences
No new dependency, thread or session format. Reuse pane-origin conversion and shared
settings controls; do not change full-layout duplication or shared help rendering.

## Validation
[Pure launch matrix and captured-request tests](../../../../nebula_app/src/gpui_shell/workspace/splitting/tests.rs)
and two window smoke tests cover resolution, live shortcuts and cancellation.
## Supersedes
None; earlier unmerged drafts are consolidated here.
## Revisit when
Factory defaults or interactive/API source semantics are deliberately changed.
