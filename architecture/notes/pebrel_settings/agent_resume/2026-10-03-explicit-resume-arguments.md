# Explicit automatic Agent resume arguments

## Status
Proposed for review, 2026-10-03.

## Context
[Issue #323](https://github.com/Kuddev/pebrel/issues/323) requests explicit per-Agent
arguments when restarting Pebrel, while preserving the default resume command for
users who have not configured them. The request does not establish a permission
regression in any provider.

## Evidence
`AgentSession::resume_command` already owns saved identity validation, including
Codex rollout normalization. GPUI workspace recovery calls
`TerminalView::restore_agent`, which waits for a new shell prompt before submission.
Pi resolves a native session file asynchronously before constructing its command.
`RuntimeSettings` and `SettingsPane::try_persist` already own preference loading,
persistence and the cached GPUI snapshot.

## Decision
Add eleven stable `agent_resume_args_<source>` preferences for providers with an
existing resume command. Each value is a JSON array of literal arguments;
missing/empty values and `[]` append nothing. `[""]` passes one explicit empty argument. The zero-dependency settings crate
preserves the serialized value; the application uses its existing JSON dependency
and the native quoting adapter in `platform/agent_resume`. Arrays are limited to 32 arguments and 64 KiB
quoted text; control characters are rejected. CMD rejects double quotes, `%` and
`!`, and quoted trailing backslashes are doubled for the native argv parser.
Windows PowerShell 5's legacy native argv binder additionally receives arguments
encoded through the existing Windows native argument encoder in `platform/elevation`.
PowerShell 7 uses the ordinary literal adapter. The pane's frozen native shell
program selects this distinction; guest/SSH arguments retain the guest adapter.

The GPUI Agent page edits drafts and validates them on Save, reports persistence
success/failure in place, and resets them with other preferences. Saving updates the
cached settings immediately for the next cold resume or retry. A command already
queued for submission retains its original snapshot. Automatic cold recovery,
including resolved Pi targets, appends arguments after the existing identity.
Invalid serialized arguments or unsupported shell quoting fail the recovery without
silently retrying a different command. The existing recovery UI retains the target.

## Rejected alternatives
Full command templates need a new substitution language and executable policy.
Replaying launch history may reproduce permissions the user never opted to resume.
Interpreting a raw shell suffix would execute expansions rather than literal args.
New parsing dependencies in the settings core violate its existing boundary.

## Consequences
No arguments or permission flags are enabled by default. Session identity and cwd
remain owned by existing recovery; session snapshots do not acquire launch history.
The setting applies to GPUI automatic recovery and retry, independent of hook
installation. Manual palette resume, forks and runtime Agent start retain their
existing commands. Legacy shell UI does not expose or apply this additive setting.
The new keys remain outside the existing WebDAV sync allowlist because arguments
can contain machine-specific values; this change does not broaden sync policy.

## Validation
Authored settings round-trip/reset tests, literal shell-quoting tests, rendered
Save/reopen/invalid/clear coverage and real cold recovery/PTY submission/retry tests.
All formatting, architecture, compile and native tests must run in GitHub Actions;
local inspection is not execution evidence. A native Windows test compiles a
small stdlib argv receiver, then runs the production adapter's generated command
through Windows PowerShell and PowerShell 7 and compares the complete argv array.
The [fork native argv experiment](https://github.com/WilliamWang1721/pebrel/actions/runs/37110686203)
confirmed that ordinary single quotes alone lose embedded quotes and argument
boundaries in Windows PowerShell 5. Native screenshot and real provider CLI
acceptance remain separate from the regression tests.

## Supersedes
None.

## Revisit when
A concrete wrapper executable/template or manual-resume configuration requirement
is accepted, or a supported shell's literal argument contract changes.
