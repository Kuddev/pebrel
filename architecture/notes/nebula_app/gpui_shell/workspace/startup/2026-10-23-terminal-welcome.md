# Terminal startup and welcome tab ownership

## Status

Proposed for review.

## Context

[Issue #702](https://github.com/Kuddev/pebrel/issues/702) asks for an optional
session chooser when opening ordinary windows and tabs. Direct shell startup
remains the default. A chooser must not start a shell in the background or lose
an explicit shell, command, working directory, or restored session identity.

The workspace owns live tabs and their Runtime identities. Settings owns the
persisted preference. Local and SSH launchers already own their respective
terminal initialization and failure presentation.

## Evidence

- [Startup dispatch](../../../../../../nebula_app/src/gpui_shell/workspace/startup.rs)
  previously lived in the workspace constructor alongside the new-tab action.
  Its restore-first and explicit-launch branches remain separate.
- [Resident handover](../../../../../../nebula_app/src/runtime_api/server.rs)
  uses the same `tab.new` and `window.create` requests as automation. Changing
  their default behavior would remove the pane an automation client expects.
- Runtime parameters reject unknown fields. Sending a new option unconditionally
  to an older resident would reject an otherwise valid launch.
- A real PTY can wake GPUI's deterministic test scheduler from its reader thread.
  Headless UI tests therefore use a scoped, thread-local spawn-failure fixture;
  native product acceptance exercises actual process startup separately.

## Decision

Keep `terminal_startup=shell|welcome` in `nebula-settings`, with missing, invalid,
and reset values resolving to `shell`. It is sampled for subsequent ordinary
windows and tabs. Existing tabs are not converted when the preference changes.

Represent the page as its own `WorkspaceTab::Welcome`. It owns its view,
subscription and optional working-directory snapshot, with no terminal pane.
Both choices use existing launch entry points. Local uses the configured local
shell; SSH opens the existing host list without copying host or credential state.

Cross-window transfer keeps the live view, captured directory and tab metadata.
The drag payload freezes the Runtime tab ID so a reorder cannot redirect the
transfer and welcome requires no invented pane identity. The receiving workspace
rebinds the event subscription and owns subsequent choices. Welcome pages attach
as separate tabs; only terminal tabs are eligible for split docking.

After a launch entry creates a terminal tab, startup orchestration adopts it into
the welcome tab's slot. It preserves the original Runtime tab ID, custom metadata
and position while taking the new terminal's launch identity. Event source entity
identity prevents a retired welcome view from starting a second session.
Connection or process failures remain visible in the terminal's existing UI.

Saved terminal sessions restore first. Welcome pages are omitted from the existing
terminal-only session format. Explicit shell/command launches, Quick Terminal,
and ordinary automation keep their direct terminal paths.

Add an opt-in Runtime parameter guarded by `window.startup_setting`. Normal
resident handover queries this capability only when welcome is selected and no
shell is explicit. Unsupported or failed discovery omits the option; the mutating
request is sent once. Requests combining the option with an explicit shell fail
validation. A welcome result has a null pane ID, never a synthetic terminal ID.

## Rejected alternatives

- Create a terminal behind an overlay: runs shell initialization before the user
  chooses a host and creates unnecessary processes and terminal subscriptions.
- Change the default semantics of `tab.new`: breaks clients that immediately use
  the returned pane for a command or agent.
- Retry a failed create without the option: an ambiguous response could create
  a second tab or window. Read-only capability negotiation avoids that replay.
- Duplicate SSH selection inside the welcome view: introduces another owner for
  host search, authentication and saved-host behavior.
- Add a second session persistence format: the welcome page has no connection or
  running work to restore; the existing terminal snapshots remain sufficient.

## Consequences

Enabling welcome defers local or SSH work until a choice is made. An older resident
still opens a direct terminal until restarted into a build advertising the feature.
Welcome-to-local creation retains the existing default-shell and directory rules.
Cancelling the host list leaves the same welcome tab available.

The startup policy is now a cohesive workspace module instead of growing the
workspace constructor. No production dependency or background service is added.
The Runtime schema remains version 1 with additive capability negotiation.

## Validation

[Behavioral fixtures](../../../../../../nebula_app/src/gpui_shell/workspace/startup/tests.rs)
cover actual controls, keyboard navigation, narrow Chinese layout, tab identity,
retired events, restore/explicit paths and opt-in Runtime tab creation.
[Cross-window fixtures](../../../../../../nebula_app/src/gpui_shell/workspace/windowing/welcome_transfer_tests.rs)
cover tear-out, live view and metadata preservation, separate-tab attachment,
stale drag payloads and event ownership after transfer.
[Native fixture](../../../../../../nebula_app/src/gpui_shell/workspace/startup/native_tests.rs)
provides repeatable desktop review with owned shutdown and isolated configuration.
Settings and Runtime tests cover default/reset/round-trip behavior and invalid
parameter combinations; both request methods are checked against the v1 schema.
Headless launch-failure fixtures do not establish real PTY or SSH connectivity.
Cross-platform native results and actual product startup evidence belong in the PR.

## Supersedes

None.

## Revisit when

The welcome page gains durable user state, additional session types, or a Runtime
client needs to choose an entry remotely. Preserve explicit-launch identities and
negotiate new protocol behavior before changing existing request defaults.
