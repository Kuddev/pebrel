# Agent activity authority across local and SSH panes

## Status

Implemented on 2026-09-19; reviewed with the terminal activity repair. This note
records ownership and fallback decisions, not a release or live-provider claim.

## Context

Separate UI state machines let screen text reopen attention after a hook had
completed a turn. Ordinary output containing `y/n` could look like a blocking
question, while completed shell commands could leave foreground identity or
progress behind. A remote pane adds another boundary: local process absence and
output silence cannot establish that its remote command finished.

## Evidence

The [shared lifecycle and regressions](../../../../nebula_app/src/ai_hook/lifecycle.rs)
cover conflicting hook/screen observations, completion and input barriers.
[Provider contracts](../../../../nebula_app/src/ai_hook/bridges.rs) distinguish
complete lifecycle/attention bridges from partial and legacy completion bridges.
[UI regressions](../../../../nebula_app/src/gpui_shell/terminal/view/activity_tests.rs)
exercise command, Agent and progress transitions together.

## Decision

One pane activity owner decides state. Provider adapters normalize bounded facts;
UI adapters validate routing and ownership, pass the shared ordering gate, apply
the event and project its effects. A rejected observation cannot advance ordering.

Capabilities belong to the bridge actually installed. Complete hooks own the
states they report. A partial bridge may use a current input control near the
prompt only for a capability it lacks; ordinary output words have no authority.
This preserves genuine questions from partial bridges without allowing loose
screen matches to override a complete hook. Native event contracts must be
declared, and a real native event suppresses legacy notify duplicates per session.

Shell completion clears foreground identity, Agent state and progress together.
Local process-probe errors remain unknown; child absence alone cannot finish a
shell builtin. Remote completion requires a remote lifecycle event, OSC 133 or a
pending command's known empty prompt. Silence, BEL and host-process absence are
not remote completion evidence.

Remote installation uses the same owned edit policy as local installation.
[The planner](../../../../nebula_app/src/ai_hook/remote.rs) owns edits and removal;
[SSH orchestration](../../../../nebula_app/src/ssh_session/integration.rs) owns
authenticated exec/PTY operations. Per-channel tokens are passed at startup and
not persisted in remote files or made dependent on AcceptEnv. Remote identity is
PID plus start epoch, separate from host PIDs. Foreign, subagent and late-session
events are rejected before ordering; compaction preserves the active turn.

## Rejected alternatives

- More per-view flags would preserve competing state authorities.
- Treating every provider as a complete hook provider would lose real questions
  from partial bridges; accepting every screen match would recreate false alarms.
- Remote silence and local process snapshots cannot distinguish remote work from
  a finished command and therefore cannot replace remote lifecycle evidence.
- Replacing whole user settings or trust databases would exceed integration
  ownership and make removal destructive.

## Consequences

Remote manifests record owned hook groups, chained notify settings and asset
hashes. Setup locking, compare-and-swap writes and rollback protect user edits;
removal restores owned settings and persists an opt-out. Integration rejection
falls back to an ordinary SSH shell, with a fresh channel after rejected exec.
Existing hook trust reviews and explicit opt-outs remain authoritative.

Complete-hook sessions skip screen classification. Partial sessions inspect the
existing bounded tail. Remote senders and event caches have bounded inputs,
ordering state and execution lifetimes. No new crate or long-lived service is
introduced. Supported shell/Python details remain authoritative in the installer
and its tests rather than a second documentation inventory.

## Validation

Tests cover lifecycle authority, replay/late events, session isolation, native and
legacy notify coexistence, install conflicts, rollback and removal. Remote tests
exercise exec fallback, token routing, controlling-TTY delivery and shell prompt
state preservation. Both UI adapters consume the shared rules. Live authenticated
model turns and visual acceptance remain separate from these protocol and state
tests.

## Supersedes

None. This is the initial module-scoped record for the repair.

## Revisit when

A provider's verified hook contract changes, or a shell gains stronger lifecycle
signals. Missing remote identity still cannot authorize a session switch using
only cwd or screen text.
