# Direct program launches in resident tabs

## Status

Implemented for review with issue #618 and PR #828.

## Context

The top-level `--command` can start a program in a new process/window. The
existing `tab.new` operation could choose a directory and shell id but could not
carry a direct program with arguments. Scripts need the same launch capability
in an explicitly selected or most recently active resident window.

## Evidence

The original request builder classified WSL from an explicit `wsl:` id or program
name before the resident resolved its actual default or imported profile. On
Windows, `/home/guest/project` was therefore made into a host-drive path for an
opaque WSL profile or a WSL default. The resident could no longer recover the
original guest directory. Its live settings can also differ from the caller's
saved settings, so resolving only in the CLI is insufficient.

The [shared CLI arguments](../../../../pebrel_app/src/cli/tab.rs),
[request builder](../../../../pebrel_app/src/runtime_api/cli.rs), and
[resident launch adapter](../../../../pebrel_app/src/gpui_shell/workspace/shell_launch.rs)
are the relevant boundaries.

## Decision

Expose `pebrel tab new` and extend `ctl new-tab` with shared launch arguments.
`tab.new` accepts optional direct argv, mutually exclusive with its shell id.
Reuse argv validation and the [literal command identity](../session/2026-11-17-literal-program-launches.md).

The CLI resolves ordinary relative directories against its own current directory.
For a Windows POSIX-rooted path it preserves `cwd` and captures its native absolute
interpretation in optional `host_cwd`. The resident resolves the actual program
first: WSL uses `cwd`, while a native program uses the captured host alternative.
The pair is structurally validated before dispatch; directory strings containing
NUL are rejected. Directories without UTF-8 representation fail at the CLI JSON
boundary instead of panicking during serialization.

Existing-window dispatch and creation of a new tab's window use one preparation
path. A WSL directory that the existing startup parser cannot represent fails
before tab/window creation. `window.create` keeps its existing request contract.
Unix programs named `wsl` retain native cwd and literal arguments.

Window selection, PTY ownership, error retention, session persistence and window
activation keep their existing owners. A successful response acknowledges tab
creation; a failed program spawn remains visible with its terminal diagnostics.

## Rejected alternatives

- Following a default-shell tab creation with injected command text creates a
  readiness race and loses literal argument boundaries.
- Guessing WSL solely from the id prefix misses imported profiles and defaults.
- Reading the caller's saved default cannot establish the resident's live default.
- Discarding the original path before IPC makes guest intent unrecoverable.
- Retrying an unsupported request without new fields could launch in a different
  directory or with a different program.

## Consequences

No new dependency, background service or persisted environment value is needed.
Old requests without new fields keep their existing inputs; older runtimes reject
`argv`/`host_cwd` rather than partially executing them. `host_cwd` requires a rooted
POSIX `cwd` and an absolute native path. Explicit window ids override the
configured preference to create another window.

## Validation

Parser tests cover literal argv, conflicting options, legacy requests, native
non-UTF8 filenames and invalid directory pairs. Routed tests use the real CLI
builder, request parser, stored profiles and live default settings, across
existing-window, no-window and new-window routes. Invalid WSL values must leave
window creation unchanged. Another test launches a native process through the
PTY and compares its actual cwd and argv, including empty and quoted values.

Settings and cwd changes are isolated in fresh child processes. Platform-specific
path construction remains in the platform adapter; ordinary routing tests run
against each native target. Production CLI generation checks the completion
snapshots on Linux/macOS and Windows.

## Supersedes

None.

## Revisit when

The Runtime introduces a unified typed launch request for tabs and windows, or
creation acknowledgements become asynchronous startup-result receipts.
