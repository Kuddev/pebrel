# Common command arguments and connection destinations

## Status

Implemented; native platform validation is recorded with the change.

## Context

Generic filesystem candidates are wrong for an SSH destination, WSL distribution,
grep pattern, or port number. A Windows path must also never fill a guest argument.
These decisions belong to the shared completion pipeline, regardless of UI mode.

## Evidence

- [OpenSSH configuration](https://man.openbsd.org/ssh_config) permits Include
  patterns relative to the user's `.ssh` directory and executable Match conditions.
- [WSL commands](https://learn.microsoft.com/windows/wsl/basic-commands) distinguish
  registered distributions, import/export host paths, and guest command lines.
- Local OpenSSH 9.5 chooses the last repeated `-F`; local WSL rejects attached
  `-dDebian` and `--distribution=Debian`. The semantic fixtures preserve these rules.
- PowerShell's Unix compatibility policy leaves `ls`, `cp`, `mv`, `rm`, and `cat`
  to native tools. CMD builtins have slash options; shell quoting is independent
  from those command argument roles.

## Decision

Keep command metadata and argument roles in `nebula-completions::semantic::common`.
The application owns connection discovery and a per-pane, two-second cache, with
the same cancellation and generation invalidation as other completion sources.
No additional executor or background service is introduced.

Reuse the SSH config tokenizer and literal-host validator in `ssh::hosts`.
Include traversal has a 1 MiB total read budget, 64-file and eight-level limits,
4096 examined directory entries, 1024 aliases, and a cooperative 250 ms deadline.
Blocking filesystem calls may exceed this deadline; all completion I/O remains on
the existing worker. Cancellation discards partial results. Includes are sorted
lexically; patterns/negated Host declarations are never invented as destinations.
Runtime-dependent Include tokens and Match conditions are not evaluated.

Add `glob` 0.3.4 as a direct dependency, reusing the version already in Cargo.lock.
Only its pattern matcher is used: directory enumeration stays bounded here rather
than allowing a library iterator to scan and sort an unbounded directory first.

WSL registry discovery belongs to `platform::shell`. It does not start a guest;
the picker retains its own plumbing-distro filter. Non-Windows hosts return no
registered WSL destinations. SSH/WSL sessions reuse remote directory demand and
never query the host connection cache. An execution snapshot identifying WSL
overrides a stale local environment label before any source selection.
Typed nested shells without a verified directory channel retain their scoped
history for path arguments. An unavailable path source must not hide that history
or fall back to the host filesystem; the existing issue-353 regression covers it.

## Rejected alternatives

- Running `ssh -G` during discovery: Match exec can run user code on every request.
  The native fixture uses `-G` only after Enter, against isolated inert test config.
- Starting WSL to list names: unnecessary cold-start work on the completion path.
- Sharing host connection/project data with guests: same spelling does not imply
  the same filesystem or configuration.
- A generic command plugin framework: the current metadata needs no new lifecycle.

## Consequences

All three UI modes share the same candidates and quoting. Explicit option values,
SSH login prefixes and jump chains retain their argument text. Unknown runtime
shell expansions stay unsupported. Guest SSH configuration/known_hosts discovery,
online WSL install catalogs, and guest paths for `wsl --cd` are not implemented by
this change. The source does not claim to reproduce OpenSSH configuration resolution.

## Validation

Existing semantic, application and native completion fixtures cover argument
roles, UTF-8 quoting, Include cycles/limits, host isolation, cancellation, three UI
modes and actual command effects. A runnable WSL fixture checks the real guest
directory through the application pipeline and executes the accepted file path.
Per-platform execution results and desktop limitations belong to PR evidence.

## Supersedes

None.

## Revisit when

Guest connection metadata has an authenticated, owned query boundary, or command
metadata grows enough to require another concrete responsibility split.
