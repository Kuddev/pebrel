# One explicit source for interactive split launches

## Status
Proposed for maintainer review in PR #484.

## Context
Splitting beside WSL or SSH needs focused inheritance and a deliberate local-shell choice.

## Evidence
[Issue #372](https://github.com/Kuddev/pebrel/issues/372) describes focused inheritance;
[PR #351](https://github.com/Kuddev/pebrel/pull/351) supplied the adapted WSL rules.
[The three-source proposal](https://github.com/Kuddev/pebrel/pull/484#issuecomment-6003205050)
and [owner review](https://github.com/Kuddev/pebrel/pull/484#issuecomment-6003895446) record the earlier choices and proposed Default factory setting.
After Windows manual checks of host/WSL/SSH splits, Ask cancellation and stable
Runtime pane IDs, the contributor requested Focused as the factory default to
keep ordinary splits in the current environment and reduce learning cost.
Maintainer product acceptance and macOS login-shell acceptance remain pending.

## Decision
Persist `split_shell_source=default|focused|ask`; missing or invalid values use Focused.
Resetting preferences also selects Focused; explicitly saved Default or Ask remains unchanged.
Declaration, value and display order are Default → Focused → Ask.
Default resolves the configured Shell/Profile and inherits only a host-visible cwd;
without one, a profile retains its startup cwd. Focused copies the pane's frozen
Shell/Profile/SSH identity and live cwd using the existing guest/user rules.
Ask reuses the launcher, capturing pane id and direction; cancel creates nothing,
and a closed source is never replaced by the new focus. Settings apply immediately
to subsequent interactive requests. Runtime API splits remain immediate Focused.

## Rejected alternatives
An implicit boolean cannot express three choices. Factory Default can leave the
focused environment for the configured shell, adding an extra choice to ordinary
splits. Rewriting explicitly saved preferences would override user intent.
Tab metadata cannot identify a mixed-shell pane.
Retargeting after source closure or waiting for a picker in the API breaks caller intent.

## Consequences
No new dependency, thread or session format. Reuse pane-origin conversion and shared
settings controls; do not change full-layout duplication or shared help rendering.
Focused SSH splits create another connection to the same destination rather than
returning to the configured local shell; this remains a product compatibility change.

## Validation
[Pure launch matrix and captured-request tests](../../../../nebula_app/src/gpui_shell/workspace/splitting/tests.rs)
and two window smoke tests cover resolution, live shortcuts and cancellation.
## Supersedes
None; earlier unmerged drafts are consolidated here.
## Revisit when
Factory defaults or interactive/API source semantics are deliberately changed.
