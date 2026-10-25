# WSL startup directory ownership

## Status

Proposed in the fix for #664.

## Context

A new WSL terminal inherited the Windows launcher's working directory unless
an earlier caller supplied a guest directory. Host existence checks also rejected
explicit `/home/...` paths before WSL could interpret them. Directory selection
crosses CLI handover, workspace launch snapshots and the final PTY boundary.

## Evidence

- [Microsoft's WSL command help](https://github.com/microsoft/WSL/blob/master/localization/strings/en-US/Resources.resw)
  defines `--cd ~` as the selected Linux user's home, `/...` as an absolute Linux
  path, and other arguments as absolute Windows paths.
- [Directory policy and regression tests](../../../../nebula_app/src/platform/launch_directory.rs)
  cover inherited host directories, guest paths, configured paths and explicit
  WSL directory/command options.
- [Workspace launch tests](../../../../nebula_app/src/gpui_shell/workspace/shell_launch/directory_tests.rs)
  exercise the snapshot-to-PTY boundary, including paths with spaces and
  relative host paths.

## Decision

The application launch adapter owns path classification. Host paths retain
existence checks and become absolute; explicit guest paths reach WSL without a
host filesystem check. Guest paths are never passed as the host process cwd.

Explicit workspace/profile directories are encoded through the existing WSL
argument parser, retaining distribution, user and guest-command boundaries.
At the final Windows PTY boundary, a launch without a selected directory uses
the configured startup directory or `--cd ~`. A direct guest command without
an explicit directory retains its existing behavior. Resident handover and cold
startup do not relabel the launcher's inherited host cwd as an explicit request.

Same-guest duplication and restoration select their guest directory before this
fallback. The default home must not erase that verified context.

## Rejected alternatives

- Adding `--cd ~` while resolving a shell identity: later duplication would see
  an explicit directory and could no longer inherit the focused guest cwd.
- Removing host validation everywhere: invalid local directories would reach
  unrelated native process creation paths.
- Checking guest directory existence through a subprocess in UI startup:
  introduces blocking guest startup and still races actual process creation.
- A second WSL argument parser: risks changing distribution/user parsing and
  arguments after `--exec` independently of the existing authority.

## Consequences

Invalid guest paths are reported by the selected WSL distribution. A leading
single slash identifies a guest path only for a resolved WSL launcher; UNC and
native paths retain their host semantics. No persistent schema, new dependency
or background service is introduced.

## Validation

Policy, resident-handover and workspace regression tests run without a registered
guest. Native Windows CI is separate from executing an installed WSL distribution;
Linux policy tests alone do not establish real guest startup behavior.

## Supersedes

None.

## Revisit when

WSL changes its startup argument grammar, or explicit directory provenance becomes
part of the shared launch schema rather than an encoded shell snapshot.
