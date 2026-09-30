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

`wsl.exe -- …` hands the joined line to the guest's login shell: measured on
2026-09-30, a directory named `x$(touch /tmp/pwned)` ran the `touch` through
`--` and not through `--exec`. Direct exec also keeps `find -printf`'s single
backslash.

## Decision

A pane's WSL identity is the distribution resolved at spawn
(`shell_detect::wsl_spawn_distro`): the explicit distribution, else the registry
default; `--distribution-id` and `--system` resolve to `None` rather than to the
default. `shell_detect::spawn_shell` pins every WSL spawn to it
(`wsl_args_pinned`) while the persisted launch stays as the user configured it,
and a PTY-default `shell=wsl` pane now spawns the snapshotted `wsl.exe`
explicitly. The pane's `PaneExecContext` records the pinned options, so the
workspace WSL location and prompt-path links read the focused pane's snapshot
(`TerminalView::wsl_distro`) instead of the tab launch. Completion scoping
reuses the spawn's value; a new-tab decision reads it on its own.
`wsl_launch_distro` keeps its explicit-only semantics.

One parser, `shell_detect::wsl_options`, reads WSL's option region for every
reader, and `is_wsl_launcher` is the one WSL program detector for launches. The
parser tolerates the `=` forms that `wsl_args_with_directory` already preserved,
although `wsl.exe` itself rejects them. Completion classifies a typed command
word separately.

An injected guest cwd is encoded for `wsl.exe`'s own command-line splitting,
not the CRT's (`shell_detect::wsl_raw_arg`). Measured on WSL 2 on 2026-09-29:
`wsl.exe` pairs `"` and keeps every backslash literal, so a CRT `\"` ends the
quote and the rest of the path runs as a guest command. A path with whitespace
is wrapped in quotes; a path containing `"` has no encoding and is not
injected: a split, duplicate or fork keeps the launch's own `--cd` or `~`, and
only a file-tree terminal starts without `--cd`. `pane.exec` in a WSL pane goes
through `std::process::Command`, whose CRT quoting has the same flaw, so it
refuses a guest cwd or argument containing `"` (`wsl_accepts_arg`, the one rule
both paths use). Persisted WSL launch arguments follow the raw convention too:
a spaced `--cd` value is stored quoted, which is what a restored raw spawn needs.

Host-side guest helpers (the side panel's git and `find`, the merge tab's
`cat`/`tee`/git) start through `shell_detect::wsl_exec_command`
(`wsl.exe -d <distro> --exec`), because the snapshot made every WSL pane, not
only an explicit `-d` one, feed its reported cwd to them. A cwd that fails
`wsl_accepts_arg` is not handed to the helpers.

Copies of a pane follow its snapshot through one rule
(`tab_duplication::copy_launch`). Split, duplicate and AI-session fork insert
`-d <snapshot>` into a bare launch, after a leading `~`, so a later default
change cannot move the copy; the pin is persisted only in the copy, and the
restored original follows the default again. The guest cwd travels through
`--cd` only into the same distribution as the same user; otherwise the copy
gets the host-visible cwd. A guest path maps to a host directory only from
`/mnt/<drive>`: Windows would resolve `/` against the current drive, and a UNC
probe would block the UI thread. The `tab_duplication` and prompt-path
(`osc_links`) tests hold the cases.

## Rejected alternatives

- Read the registry default whenever a location is needed: a later default
  change would silently retarget a running pane — the guess the old rule forbade.
- Rewrite bare launches to `-d <default>` in the persisted launch: changes
  launch identity and restore semantics for users who intentionally follow the
  default. Only the spawn options are pinned.
- Take the identity from the guest's `WSL_DISTRO_NAME`, which the bash and zsh
  reports carry in the `pebrel_shell` token and completion uses to fill an empty
  distribution: it arrives only at the first prompt, after an early split, and
  never from shells without the integration, so it supplements the snapshot.
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

- Registry-free tests in `shell_detect` (resolution, option region, launch
  rewriting, spawn composition), `tab_duplication` (copies), `osc_links`
  (prompt paths) and `runtime_exec` (`pane.exec` refusing `"` in a guest cwd or
  argv).
- By hand, not automated: the `"`/backslash encoding against a real `wsl.exe`,
  and interactive file-tree following.

## Supersedes

None. Narrows the explicit-only rule documented at `wsl_launch_distro`.

## Revisit when

WSL exposes the running distribution of a process, or panes gain an OSC-reported
guest identity.
