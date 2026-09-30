# WSL zsh guests receive the shared zsh bootstrap through translated ZDOTDIR

## Status

Proposed for review with implementation.

## Context

WSL panes report command boundaries and cwd through a `PROMPT_COMMAND` sent in
`WSLENV` (`shell_detect::wsl_cwd_report_env`). Only bash reads that variable. A
guest whose login shell is zsh reported no OSC 7, so the file tree, Git view,
split/duplicate directory inheritance and prompt-path links stayed at the
initial directory. `wsl.exe` must keep launching the guest login shell; adding
`--exec` was the 1.1.0 regression, so no startup argument can be injected.

## Evidence

Local zsh already uses a `ZDOTDIR` bootstrap (`res/shell/zshenv`, `zprofile`,
`zshrc`) that restores the original `ZDOTDIR`, sources the user's own files and
then installs precmd/preexec reports. The rc already derives its shell token
from `WSL_DISTRO_NAME`. `WSLENV` `/p` translates a Windows path into the guest
automount path, so a host directory can be named without writing into the guest.

Unlike local zsh, the WSL variables reach every guest process, including
`wsl <cmd>`, which WSL runs as `$SHELL -c <cmd>`. A real-zsh reproduction
(`test_non_interactive_zsh_does_not_leak_the_bootstrap_zdotdir`) showed the
earlier bootstrap exporting the host `ZDOTDIR` and `NEBULA_ZDOTDIR_WAS_SET=1`
from `zsh -c`; a nested zsh then skipped the user's XDG-layout startup files.

Review of the first cut found three more leaks on a real Ubuntu 26.04 guest,
each reproduced in the Python suite: `/etc/zsh/zshrc` ran `compinit` while
`ZDOTDIR` still named the bootstrap and wrote `.zcompdump` into the shared host
directory; zsh's `newuser` check looked in the bootstrap and never offered the
wizard; `NEBULA_*` stayed exported into programs the user's rc files exec'd. A
second review found compinit replayed with `GLOBAL_RCS` off or
`skip_global_compinit` set in `~/.zprofile`, and the newuser check running
after the user's `.zshenv` had moved `ZDOTDIR`.

Maintainer review found two startup regressions no guest-side script can
repair, because they happen before or without any zsh reading the bootstrap:
`ZDOTDIR` was replaced before the guest could verify that the bootstrap is
reachable (`[automount] enabled=false`, a failed `/p` translation, drvfs
permissions for another `-u` user), and zsh, which falls back to `$HOME` only
when `ZDOTDIR` is unset, then skipped the user's own startup files; and an
unspecified guest command was taken as proof that the login shell is zsh, so a
fish or nushell login kept a host `ZDOTDIR` that nothing restores.

## Decision

- **Host side.** The host writes the same three files, CR-normalized, into
  `<data dir>/shell-integration/wsl-zsh`, only when that directory is on a
  fixed local drive, once per process and only on the probe worker. A failed
  write is retried after five minutes. A pane that takes over adds
  `ZDOTDIR/pu`, `NEBULA_ZSH_INTEGRATION/pu` and `NEBULA_ZDOTDIR_WAS_SET/u` to
  `WSLENV`.
- **Guest command.** A launch that runs a guest command other than zsh
  (`wsl htop`, `wsl sh -c …`, `-e bash`) receives none of the zsh variables and
  is never probed: no zsh would take them back, and an installer that finds
  `ZDOTDIR` would write its `.zshrc` into the host bootstrap.
- **The guest answers, not the host.** Before the first takeover of a
  (distribution, user), `platform::wsl_guest_shell` runs one
  `wsl.exe --distribution <d> [--user <u>] --exec sh -s` with
  `res/shell/wsl-guest-probe.sh` on stdin and the bootstrap sent through `WSLENV`
  `/p`. The guest reports the login shell from its passwd entry (`getent`, else
  `/etc/passwd`) and whether that user can read all three files there. The
  verdict is cached per process; a failed probe is retried after five minutes.
  A login shell takes the bootstrap only when the guest said zsh **and**
  readable, an explicit `-e zsh` only when readable. Unknown (failed, timed
  out, or `--distribution-id`/`--system` with no name to enter) leaves the
  guest environment untouched.
- **No wait on the UI thread.** The spawn (`nebula_app/AGENTS.md`: no waits on
  disk or child processes) reads only the cached verdict and an already written
  bootstrap (`wsl_zsh_directory_ready`, a `try_lock` and three stats). A first
  sight starts the worker, which writes the files and then probes; that pane
  starts without zsh reports. `main` warms the first pane's guest at process
  start, resolving the shell with the pane's own authority
  (`shell_launch::startup_shell`), and skips it when a command was given. The
  worker uses `platform::process::run_bounded`, shared with the hook installer.
- **Opt-out first.** A host `WSLENV` that already forwards `ZDOTDIR` is checked
  before any probe or write (`shell_detect::wslenv_forwards`), and zsh is left
  alone. The host cannot observe the guest's original `ZDOTDIR`; it is unset.
- **Nested guests.** WSL forwards every variable `WSLENV` names to a `wsl.exe`
  started inside the guest, whatever the `/u` flag, so the bootstrap `.zshenv`
  (every zsh) and the bash first prompt remove the three entries from the
  guest's `WSLENV` once read.
- **Takeover scope.** The bootstrap `.zshenv` takes over only an interactive
  shell (`-o rcs && -o interactive`). A non-interactive zsh restores the user's
  `ZDOTDIR` and unsets the `NEBULA_*` variables. When taking over, it keeps the
  `NEBULA_*` state unexported, and each restore keeps the export attribute the
  user gave `ZDOTDIR`, so a child zsh still starts from `~/.zshenv`.
- **Global compinit.** While taking over, `.zshenv` sets `skip_global_compinit`
  to a marker value unless the user did. If the marker survives the user's
  `.zprofile`, the bootstrap `.zshrc` clears it and, with `GLOBAL_RCS` still on,
  repeats Ubuntu's own condition, so `compinit` runs once the user's `ZDOTDIR`
  is back.
- **New users.** The bootstrap `.zshenv` checks for the four startup files
  before sourcing the user's `.zshenv`, where zsh itself checks; `.zshrc` then
  runs `zsh-newuser-install`.
- **One bootstrap, two guest callers.** The file writer is shared with local
  zsh. The hook installer keeps its own guest target (an install queue, not a
  verdict cache); both name the pane's guest because the spawn options are
  pinned to the distribution snapshot.

## Rejected alternatives

- `--exec zsh`: replaces the user's chosen login shell (the 1.1.0 regression).
- Write rc files into the guest through `wsl.exe`: an extra subprocess and
  guest mutation on every pane spawn, with a race against shell startup.
- A second WSL-specific zsh script: two diverging report implementations.
- Rewrite the files on every spawn: fsyncs on the UI thread and a replace that
  can race a guest zsh that is still sourcing them.
- Forward the zsh variables for every guest command and rely on the interactive
  gate in `.zshenv`: a command without zsh (an `sh -c` installer) keeps the
  host `ZDOTDIR` for good.
- Treat an unspecified guest command as zsh, or hand out `ZDOTDIR` without a
  reachability check (the submitted cut): the two regressions above.
- Read `\\wsl.localhost\<distro>\etc\passwd` from the host instead of probing:
  it also starts the distribution and blocks, but cannot see a failed `/p`
  translation or another user's drvfs permissions.
- Wait on the UI thread for the first verdict, even bounded: session restore
  creates several WSL panes in a row, each waiting while a cold guest boots.
- Defer the PTY spawn until the verdict: the view would need a pending state
  and an asynchronous attach; out of scope for this change.
- Delete `.zcompdump` from the bootstrap after the global rc: the dump would be
  rebuilt on every start, which is slower than skipping and repeating compinit.
- Re-source `/etc/zsh/zshenv` to recover a conditional system `ZDOTDIR`: it
  runs the system file twice, duplicating whatever else it sets.
- fish through `XDG_CONFIG_HOME`/`XDG_DATA_DIRS`: replacing them changes where
  fish and other programs find all user and system data. fish is not handled.

## Consequences

- **Startup cost.** An interactive guest zsh reads three small files from the
  host drive; a non-interactive one reads only `.zshenv`. Each (distribution,
  user) costs one guest `sh` round trip per process, on a worker thread. With
  a WSL default shell the warm-up starts a stopped distribution at launch.
- **Panes before the verdict.** Every pane of a guest spawned before its first
  verdict (a profile other than the first pane's shell, or a whole restored
  session on a cold distribution) has no zsh reports for its lifetime; later
  panes use the cached verdict. Those panes still read their own startup files.
- **Unreadable bootstrap.** If automount is disabled (`[automount] enabled=false`),
  `/p` translation fails, or drvfs permissions hide the files from the pane's
  `-u` user, the guest reports the bootstrap unreadable and the pane keeps its
  own startup environment. UNC or redirected data directories are never offered.
- **Other login shells.** A fish, nushell or bash login receives no zsh
  variables, because the guest named it. A zsh started from another shell, or
  under a guest command such as `wsl bash`, is not integrated.
- **Windows children.** The pane's Windows-side environment keeps `ZDOTDIR`, so
  a Windows program started from the guest (`code`, `cmd.exe`) sees the host
  bootstrap path; only zsh reads it.
- **Local zsh.** The shared scripts changed local macOS/Linux zsh too: the
  interactive gate, unexported `NEBULA_*` state, the kept `ZDOTDIR` export
  attribute, Ubuntu's replayed compinit and the newuser wizard for a user with
  no startup files.
- **Global compinit elsewhere.** Only Ubuntu's `skip_global_compinit` contract is
  repeated; another distribution whose global rc runs `compinit` unconditionally
  still writes its dump into the bootstrap.
- **System zshenv.** A guest `/etc/zsh/zshenv` that unconditionally assigns
  `ZDOTDIR` wins, and the integration becomes inert. One that assigns it only
  when it is unset sees the bootstrap value, so the user's files are read from
  `$HOME` instead.
- **Stale verdict.** `chsh` in the guest, or a changed mount, is seen by the
  next Pebrel process; the bash first-prompt cleanup guards a changed passwd.
- **Failures.** A failed write or a guest that cannot run `sh` or does not
  answer in time logs one warning and is retried after five minutes.

## Validation

- **Rust unit tests.** `shell_detect` covers `WSLENV` entries, the opt-out,
  reading the guest command from WSL's option region, the spawn composition
  (`spawn_shell`: pinned bare and PTY-default WSL, untouched PowerShell) and the
  bash cleanup. `platform::wsl_guest_shell` covers the probe command, the
  parser and the takeover decision with a guest double (cheap refusals never
  probe; readable plus zsh required; bootstrap looked up last).
  `platform::process` covers `run_bounded` (stdin, exit status, time budget,
  output cap). `shell_launch` checks that warm-up resolves bare `wsl`.
- **Real zsh.** The Python suite runs the bootstrap in real zsh for login,
  interactive and nested non-interactive shapes, global rc files, no user files,
  variables seen by the user's rc, `GLOBAL_RCS` off, `skip_global_compinit` in
  `~/.zprofile`, XDG `ZDOTDIR` exported or not (a child zsh still reads
  `~/.zshenv`), and a `WSLENV` left with no bootstrap entries. The leak,
  compinit, newuser, export and `WSLENV` cases fail against earlier scripts. The
  probe runs under real `sh` with readable, partial, missing, untranslated and
  empty bootstraps and without `getent`.
- **By hand.** The probe and a nested `wsl.exe` under a taken-over zsh were run
  against a real Ubuntu 26.04 guest; an end-to-end pane is not automated.

## Supersedes

None.

## Revisit when

WSL offers a supported guest-side startup hook, the first-pane gap is reported
often enough to justify a pending view state, or fish integration gains a
non-invasive injection point.
