# WSL pane identity is the distribution resolved at spawn

## Status

Proposed for review with implementation.

## Context

`wsl_launch_distro` recognizes only an explicit `-d`/`--distribution`; its test
states that guessing the default distribution for a bare `wsl` could point at the
wrong guest. As a consequence, bare `wsl`, legacy `shell=wsl` and panes whose
default shell is WSL had no cwd mapping: the file tree, Git view and prompt-path
links ignored them. The workspace also read WSL identity from the tab-level
launch, which is `Default` for a default-shell tab and `Profile` for imported
profiles, so those panes were ignored even with an explicit distribution.

## Evidence

`wsl.exe` itself reads `HKCU\...\Lxss\DefaultDistribution` when it starts. The
completion context already resolved that value at spawn for history scoping
(`completion_context::launch_environment`). Each view keeps its own spawn-time
`session_launch`; split panes can differ from the first pane of a tab.

Review of the first cut found that the helpers disagreed on where WSL's options
end. `wsl_launch_distro` scanned the whole argv, so `wsl -e tool -d x` named the
guest command's `x`. `--distribution=Debian` and `--system` fell through to the
registry default. Explicit shell arguments are joined raw
(`tty::Options::escape_args` is `false`), so an injected `--cd /home/a b` split
into a directory plus a guest command; a directory name from a cloned repository
could thus run a guest command on split, duplicate or fork. `runtime_exec`, the WSL hook setup and a
PTY-default `shell=wsl` pane still read launch arguments without the snapshot.

## Decision

`shell_detect::wsl_spawn_distro` returns the explicit distribution or the
registry default. A spawn reads it once and completion scoping reuses the
value; a new-tab decision and the startup warm-up read it on their own.
`--distribution-id` and `--system` resolve to `None` rather than to the default.
`TerminalView` snapshots the value as `wsl_distro`. The workspace WSL location
and prompt-path links read the focused pane's snapshot instead of the tab
launch. `shell_detect::spawn_shell` composes the spawn: a PTY-default WSL pane
spawns the snapshotted `wsl.exe` explicitly, and every WSL spawn is pinned to
the snapshot (`wsl_args_pinned`) while the persisted launch stays as the user
configured it. The pane, its `PaneExecContext`, the guest shell probe and the
hook installer therefore read the same guest from the same options.
`wsl_launch_distro` keeps its explicit-only semantics.

One parser, `shell_detect::wsl_options`, reads WSL's option region for the
distribution, user, distribution selection and guest command. It stops at `--`,
`-e`/`--exec` or the first argument it does not know. It tolerates the `=` forms
that `wsl_args_with_directory` already preserved, although `wsl.exe` itself
rejects them. A leading `~` is WSL's own directory choice like `--cd`: both
give way to an injected or inherited host directory. `is_wsl_launcher` is the
WSL program detector for launches (the snapshot, the cwd report environment,
argument rewriting, `runtime_exec`, hook setup); completion classifies a typed
command word separately.

An injected guest cwd is encoded for `wsl.exe`'s own command-line splitting,
not the CRT's (`shell_detect::wsl_raw_arg`). Measured on WSL 2 on 2026-09-29:
`wsl.exe` pairs `"` and keeps every backslash literal, so a CRT `\"` ends the
quote and the rest of the path runs as a guest command. A path with whitespace
is wrapped in quotes; a path containing `"` has no encoding and is not
injected, so the copy starts without `--cd`. `pane.exec` in a WSL pane goes
through `std::process::Command`, whose CRT quoting has the same flaw, so it
refuses a guest cwd or argument containing `"` (`wsl_accepts_arg`, the one rule
both paths use). Persisted WSL launch arguments follow the raw convention too:
a spaced `--cd` value is stored quoted, which is what a restored raw spawn needs.

Copies of a pane follow its snapshot:

- **Split and duplicate** insert `-d <snapshot>` into a bare launch, after a
  leading `~`. A later default change therefore cannot move the copy to another
  guest. The pinned argument is part of the copy's persisted launch, so a
  restored copy stays in that guest while the restored original follows the
  default again.
- **A WSL pane without a guest cwd** (fish, or before the first prompt) splits
  into the same guest rather than the host default shell, replaying its spawn:
  the launch's own `--cd` or `~` still wins over the spawn-time host directory.
- **A new default-shell tab** inherits the guest cwd only when it targets the
  same distribution and explicit user, and does not choose its own directory.
- **A duplicate or AI-session fork** uses the tab's identity, which may differ
  from the focused pane's; the guest cwd follows only into the same
  distribution and user. A pane without a snapshot qualifies when the identity
  is its own launch apart from the directory.
- **Relative prompt paths** resolve against the guest cwd mapped into the
  snapshotted distribution, like absolute ones.
- **Otherwise** a WSL pane's guest path yields only a `/mnt/<drive>` host
  directory, even without a snapshot; Windows would resolve `/` against the
  current drive, and a UNC probe would block the UI thread.

## Rejected alternatives

- Read the registry default whenever a location is needed: a later default
  change would silently retarget a running pane — the guess the old rule forbade.
- Rewrite bare launches to `-d <default>` in the persisted launch: changes
  launch identity and restore semantics for users who intentionally follow the
  default. Only the spawn options are pinned.
- Take the identity from the guest's `WSL_DISTRO_NAME`: the bash and zsh
  reports already carry it in the `pebrel_shell` token, which completion uses
  to fill an empty distribution. It arrives only at the first prompt, which is
  after a split made before it, and never from shells without the integration,
  so it could supplement the snapshot but not replace it.
- CRT quoting (`escape_args`, or the PTY's escaper on the injected value):
  `wsl.exe` does not parse `\"`, so a directory name containing `"` would still
  inject a guest command, as the first cut of this fix did. `escape_args` would
  also quote profile and persisted arguments that follow the raw convention.
- Drop the guest command (`-e htop`) when splitting: it would also drop
  shell-selecting commands such as `--exec zsh -l`, so splits keep the pane's
  command as duplicates do.

## Consequences

A default change between the registry read and `wsl.exe` start could still
mismatch; the window is the same spawn call. Restored panes resolve again at
their own spawn. SSH and host panes have no snapshot. A `--distribution-id` or `--system`
pane has no WSL location until the id is mapped to a name, and its commands run
without a distribution argument. Splitting a pane whose launch runs a guest
command runs that command again. A distribution renamed or re-imported under
another name after spawn leaves pinned copies pointing at the old name.

## Validation

- **Registry-free unit tests.** Explicit, `=`-form, bare, id-based, `--system`
  and non-WSL resolution, with an injected default, and guest-command
  arguments that must not count as WSL options.
- **Launch rewriting.** Pinning, the `~` marker, quoting of spaced paths with
  literal backslashes, refusal of paths containing `"` and `--distribution-id`.
  The encoding was checked against a real `wsl.exe`, which is not automated.
- **Pane snapshot.** Prompt-path mapping, the spawn composition
  (`spawn_shell`) and the exec context from pinned options; `pane.exec`
  refusing a `"` in the guest cwd or argv.
- **Split, duplicate and new-tab launches.** Guest cwd, no guest cwd, the
  launch's own `--cd`, another distribution or user, a host shell and a profile
  directory.
- **Not automated.** Interactive file-tree following.

## Supersedes

None. Narrows the explicit-only rule documented at `wsl_launch_distro`.

## Revisit when

WSL exposes the running distribution of a process, or panes gain an OSC-reported
guest identity.
