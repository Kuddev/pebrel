# Literal program launches and saved identity

## Status

Implemented for review while preparing PR #828.

## Context

An explicit `pebrel -e` program supplies argv, while existing detected shells
and saved profiles can carry prepared Windows command-line fragments. Both were
stored as `LaunchSession::Shell`. The Windows PTY defaults to raw fragments,
so an explicit argument containing spaces was split and an empty argument lost.
Setting native escaping on every shell would reinterpret existing bootstrap text.

## Evidence

The existing Windows command-line tests distinguish raw `echo hello world` from
the escaped `echo "hello world"` form. The application previously created default
PTY options for both explicit programs and configured shells. Its session model
could not retain a separate argument mode through restoration or duplication.
See the [launch model](../../../../pebrel_app/src/session.rs),
[native encoder](../../../../pebrel_terminal/src/tty/windows/command_line.rs), and
[restore regression](../../../../pebrel_app/src/gpui_shell/workspace/windowing/command_launch_tests.rs).

## Decision

Represent an explicit program as `LaunchSession::Command`, retaining its name,
program and literal argument vector. The GPUI launch adapter carries that mode
to the existing native PTY encoder. Configured Shell/Profile launches keep their
existing compatibility behavior. Stored shell records do not change meaning.

An explicit command retains its requested native cwd, does not receive a shell
bootstrap or welcome input, and is not a destination for automatic shell resume
text. Per-pane identity, configured environment and proxy settings still apply.
Native Windows argv escaping happens once at the PTY boundary; that mode does
not rewrite a user-specified managed bootstrap path.

WSL directory options remain separate argv elements in command mode. Guest
inheritance preserves this encoding across copies. On Unix, an explicit native
program named `wsl` keeps local execution/completion scope and receives no WSL
environment takeover; compatibility shell descriptors retain their prior policy.

The shared launch-to-terminal adapter lives in `workspace/shell_launch.rs`.
Windows command-line construction and its tests live in `tty/windows/command_line.rs`.
These responsibilities keep the existing file budgets unchanged.

## Rejected alternatives

- Enabling escaping globally would double-escape existing raw shell fragments.
- Quoting argv before shell resolution would put encoded values into WSL
  distribution/user identity and saved snapshots.
- A transient flag on the initial window request would be lost on restore/copy.
- Inferring literal mode from executable names cannot distinguish explicit
  commands from configured shell bootstraps using the same executable.

## Consequences

The v4 session format gains the `command` launch kind. Existing Shell/Profile
records remain readable. Builds without this kind cannot read such saved launches;
the legacy shell, like its serial adapter, does not restore this GPUI-only kind.
New explicit commands are restored from their own argv instead of receiving an
additional shell command inferred from a saved foreground agent.

No new dependency, process owner or persistent environment value is introduced.
The existing PTY lifecycle and per-pane environment builder remain authoritative.

## Validation

A native test executable records its cwd and arguments after ordinary startup,
file save/load and restoration. It covers spaces, empty strings, quotes,
backslashes, Unicode and literal shell metacharacters on all native platforms.
The fixture uses an isolated child and checks that saved agent metadata does not
queue shell input into the explicit program. Additional tests cover copy identity,
old shell records, WSL directory encoding, UNC cwd and managed-bootstrap refresh.

## Supersedes

None.

## Revisit when

Configured shell/profile arguments migrate from raw Windows fragments to literal
vectors with an explicit compatibility policy. Reuse this encoder and identity.
