# Direct program launches in resident tabs

## Status

Proposed for review with issue #618.

## Context

The top-level `--command` starts a program in a new process/window. The existing
`tab.new` operation could choose a directory and shell id, but could not carry a
direct program with arguments. Scripts therefore could not use that operation to
start a command in a selected resident window.

## Evidence

`cli::TabCommand` had no `new` verb, and `ControlCommand::NewTab` only exposed a
window id. `runtime_api::command` rejected an `argv` field. The pre-change local
binary rejects `pebrel tab new --help` as an unrecognized subcommand.

## Decision

Expose `pebrel tab new` and extend `ctl new-tab` with shared launch arguments.
`tab.new` accepts optional direct argv, mutually exclusive with its shell id.
Reuse the existing argv validation and program-to-launch identity conversion.
Both existing-window dispatch and window creation carry the same program, cwd,
and arguments. Relative CLI directories resolve before the resident receives them.

Keep window selection, PTY ownership, error retention, session persistence, and
window activation under their existing owners. The response acknowledges tab
creation, not command completion; a failed spawn remains visible in the tab and
its runtime state.

## Rejected alternatives

- Creating a default shell tab and following it with a separate input request:
  introduces a readiness race, shell-dependent quoting, and two independently
  failing mutations for one launch intent.
- Changing ordinary GUI startup handover: unnecessary for the explicit resource
  command and risks changing launch behavior for existing scripts.
- Joining argv into shell text: loses argument boundaries and reinterprets literal
  metacharacters.

## Consequences

No new dependency, persisted format, thread, or long-lived service is required.
Old requests omit argv and keep their behavior. Old runtimes reject the new field;
the CLI returns that failure without retrying or silently launching another shell.
Explicit window ids override the configured new-window preference.

## Validation

CLI parsing and Runtime request tests cover literal arguments, conflicting options,
invalid argv, legacy requests, and directory resolution. GPUI dispatcher tests cover
MRU and explicit routing, nonexistent windows, window creation, and retained spawn
failures. A Linux PTY test executes a real program and checks its cwd and exact argv.
The settings fixtures share the existing in-process and nextest serialization group.

## Supersedes

None.

## Revisit when

The Runtime introduces a unified typed launch request for tabs and windows, or its
response contract changes from creation acceptance to asynchronous startup results.
