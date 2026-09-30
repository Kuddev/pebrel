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

- Local zsh already uses a `ZDOTDIR` bootstrap (`res/shell/zshenv`, `zprofile`,
  `zshrc`) that restores the original `ZDOTDIR`, sources the user's files and
  installs precmd reports; its token already carries `WSL_DISTRO_NAME`. `WSLENV`
  `/p` translates a Windows path into the guest automount path.
- The WSL variables reach every guest process, including `wsl <cmd>` (run as
  `$SHELL -c <cmd>`). Against a real Ubuntu 26.04 guest the first cuts leaked:
  `zsh -c` exported the host `ZDOTDIR` so a nested zsh skipped XDG startup
  files; `/etc/zsh/zshrc` ran `compinit` into the shared bootstrap; zsh's
  newuser check looked in the bootstrap; `NEBULA_*` reached programs the user's
  rc files exec'd. Each case is reproduced in `scripts/tests/test_shell_integration.py`.
- Two failures no guest script can repair, because no zsh reads the bootstrap:
  a guest that cannot reach it (`[automount] enabled=false`, a failed `/p`
  translation, drvfs permissions for another `-u` user) still had `ZDOTDIR`
  replaced, and zsh then skipped the user's files (it falls back to `$HOME`
  only when `ZDOTDIR` is unset); and a login shell was assumed to be zsh, so a
  fish or nushell login kept a `ZDOTDIR` nothing restores.

## Decision

- **The guest answers, not the host.** Before the first takeover of a
  (distribution, user), `platform::wsl_guest_shell` runs
  `wsl.exe --distribution <d> [--user <u>] --exec sh -s` with
  `res/shell/wsl-guest-probe.sh` and the bootstrap sent through `WSLENV` `/p`.
  The guest reports its passwd login shell (`getent`, else `/etc/passwd`) and
  whether that user can read all three files. A login shell takes the bootstrap
  only when the guest said zsh **and** readable, an explicit `-e zsh` only when
  readable. Unknown (failed, timed out, `--distribution-id`/`--system`) leaves
  the guest untouched. The verdict, with the bootstrap path, is cached per
  process; a failed probe is retried after five minutes.
- **Guest command.** A guest command other than zsh (`wsl htop`, `-e bash`) is
  never probed and receives no zsh variables: an installer that finds `ZDOTDIR`
  would write its `.zshrc` into the host bootstrap. A host `WSLENV` that already
  forwards `ZDOTDIR` opts out before any probe.
- **Host files.** The probe worker writes the three CR-normalized files into
  `<data dir>/shell-integration/wsl-zsh` once per process, and again only when
  one is missing. A taking-over pane adds `ZDOTDIR/pu` and
  `NEBULA_ZSH_INTEGRATION/pu` to `WSLENV`.
- **No wait on the UI thread.** The spawn (`nebula_app/AGENTS.md`) reads only the
  cached verdict; a first sight starts the worker and that pane starts without
  zsh reports. `main` warms the first pane's guest at process start
  (`shell_launch::startup_shell`, the pane's own resolution) unless a command
  was given. The worker shares `platform::process::run_bounded` with the hook
  installer.
- **Removal.** WSL forwards every variable `WSLENV` names to a `wsl.exe` started
  in the guest, whatever `/u` says, so the bootstrap `.zshenv` and the bash first
  prompt drop both entries from the guest `WSLENV`. `wsl.exe --exec` helpers
  never run either, so `shell_detect::strip_wsl_zsh_takeover` removes the
  takeover from `pane.exec` and Runtime git environments.
- **Takeover scope.** `.zshenv` takes over only an interactive zsh started with
  `NEBULA_ZSH_INTEGRATION`, keeps the `NEBULA_*` state unexported and restores
  the export attribute the user gave `ZDOTDIR`. A zsh started while a parent
  bootstrap still exports `ZDOTDIR` sees no companion variable and restores
  instead of taking over with an empty directory.
- **Global compinit and new users.** `.zshenv` sets `skip_global_compinit` to a
  marker unless the user did; if it survives `.zprofile`, `.zshrc` repeats
  Ubuntu's condition once the user's `ZDOTDIR` is back. The newuser check runs
  where zsh runs it, before the user's `.zshenv`.

## Rejected alternatives

- `--exec zsh`: replaces the user's chosen login shell (the 1.1.0 regression).
- Write rc files into the guest through `wsl.exe`: an extra subprocess and
  guest mutation on every pane spawn, with a race against shell startup.
- A second WSL-specific zsh script: two diverging report implementations.
- Rewrite the files on every spawn: fsyncs on the UI thread and a replace that
  can race a guest zsh that is still sourcing them.
- Forward the zsh variables for every guest command and rely on the interactive
  gate: an `sh -c` installer keeps the host `ZDOTDIR` for good.
- Guess from the host: treat an unspecified guest command as zsh, check that the
  data directory is on a fixed drive, or read `\\wsl.localhost\<d>\etc\passwd`.
  None sees a failed `/p` translation or another user's drvfs permissions; the
  probe answers both.
- Wait on the UI thread for the first verdict, even bounded: session restore
  creates several WSL panes in a row, each waiting while a cold guest boots.
- Defer the PTY spawn until the verdict: the view would need a pending state
  and an asynchronous attach; out of scope for this change.
- Delete `.zcompdump` after the global rc: it would be rebuilt on every start.
- Re-source `/etc/zsh/zshenv` to recover a conditional system `ZDOTDIR`: it
  runs the system file twice, duplicating whatever else it sets.
- fish through `XDG_CONFIG_HOME`/`XDG_DATA_DIRS`: replacing them changes where
  fish and other programs find all user and system data. fish is not handled.

## Consequences

- **Startup cost.** An interactive guest zsh reads three small files from the
  host drive. Each (distribution, user) costs one guest `sh` round trip per
  process, on a worker. With a WSL default shell the warm-up starts a stopped
  distribution at launch.
- **Panes before the verdict.** Every pane of a guest spawned before its first
  verdict (a profile other than the first pane's shell, or a restored session on
  a cold distribution) has no zsh reports for its lifetime; it still reads its
  own startup files.
- **Unreadable bootstrap.** Disabled automount, a failed translation, drvfs
  permissions or a UNC data directory make the guest answer unreadable; the pane
  keeps its own startup environment.
- **Other shells.** A fish, nushell or bash login receives no zsh variables. A
  zsh started from another shell, or under `wsl bash`, is not integrated.
- **Windows children.** The pane's Windows-side environment keeps `ZDOTDIR`, so
  a Windows program started from the guest sees the host bootstrap path.
- **Local zsh.** The shared scripts changed local macOS/Linux zsh too (the gates,
  export attribute, compinit replay and newuser wizard).
- **Limits.** Only Ubuntu's `skip_global_compinit` contract is repeated. A guest
  `/etc/zsh/zshenv` that assigns `ZDOTDIR` unconditionally makes the
  integration inert; one that assigns it when unset reads user files from `$HOME`.
- **Stale verdict.** `chsh` or a changed mount is seen by the next Pebrel
  process. A bootstrap deleted after the probe is not noticed either: later zsh
  panes of that guest skip the user's startup files until Pebrel restarts (the
  spawn no longer stats the files on the UI thread). A failed write or probe
  logs one warning and is retried after five minutes.

## Validation

- Rust: `shell_detect` (environment, opt-out, option region, spawn composition,
  bash cleanup, helper stripping), `platform::wsl_guest_shell` (probe command,
  parser, takeover table), `platform::process` (`run_bounded`), `shell_launch`
  (warm-up resolves bare `wsl`).
- Real shells: `scripts/tests/test_shell_integration.py` runs the bootstrap and the
  probe under real zsh and `sh`; the leak, compinit, newuser, export, window and
  `WSLENV` cases fail against earlier scripts.
- By hand: the probe and a nested `wsl.exe` under a taken-over zsh against
  Ubuntu 26.04; an end-to-end pane is not automated.

## Supersedes

None.

## Revisit when

WSL offers a supported guest-side startup hook, the first-pane gap is reported
often enough to justify a pending view state, or fish integration gains a
non-invasive injection point.
