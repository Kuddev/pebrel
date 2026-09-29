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
(`scripts/tests/test_shell_integration.py`,
`test_non_interactive_zsh_does_not_leak_the_bootstrap_zdotdir`) showed that the
earlier bootstrap exported the host `ZDOTDIR` and `NEBULA_ZDOTDIR_WAS_SET=1` from
`zsh -c` without `NEBULA_ORIGINAL_ZDOTDIR`. A nested zsh, such as one started by a
terminal multiplexer, then set an empty `ZDOTDIR` and skipped the user's XDG-layout startup files.

Review of the first cut found three more leaks on a real Ubuntu 26.04 guest:

- `/etc/zsh/zshrc` runs `compinit` unless `skip_global_compinit` is set, and it
  runs while `ZDOTDIR` still names the bootstrap, so `.zcompdump` was written
  into the shared host directory. Every distribution then rebuilt the others'
  dump on the drvfs mount.
- zsh's `newuser` check runs after the global zshenv and before any user
  startup file, and looks in `ZDOTDIR`. The bootstrap's files meant the wizard
  never ran for a user without files.
- `NEBULA_*` stayed exported while the user's rc files ran, so a terminal
  multiplexer they exec'd carried the variables into its server.

Each case fails against the first-cut scripts in the Python suite. A second
review found that the first fix replayed compinit even with `GLOBAL_RCS` off or
`skip_global_compinit` set in `~/.zprofile`, and ran the newuser check after the
user's `.zshenv` had moved `ZDOTDIR`.

## Decision

- **Host side.** For WSL spawns the host writes the same three files, CR-normalized,
  into `<data dir>/shell-integration/wsl-zsh`. It writes them once per process
  and only when that directory is on a fixed local drive. A failed first write
  is cached; a bootstrap file deleted while the process runs is rewritten.
  It then adds `ZDOTDIR/pu`, `NEBULA_ZSH_INTEGRATION/pu` and
  `NEBULA_ZDOTDIR_WAS_SET/u` to `WSLENV`.
- **Guest command.** A launch that runs a guest command other than zsh
  (`wsl htop`, `wsl sh -c …`, `-e bash`) receives none of the zsh variables: no
  zsh would take them back, and an installer that finds `ZDOTDIR` would write
  its `.zshrc` into the host bootstrap. The login shell or `-e zsh` still does.
- **Unknown guest `ZDOTDIR`.** The host cannot observe the guest's original
  `ZDOTDIR`, so it is treated as unset.
- **User-forwarded `ZDOTDIR`.** If the host already forwards one, Pebrel leaves
  zsh untouched.
- **Takeover scope.** The bootstrap `.zshenv` takes over only an interactive
  shell (`-o rcs && -o interactive`). A non-interactive zsh restores the user's
  `ZDOTDIR` and unsets the `NEBULA_*` variables. When taking over, it keeps the
  `NEBULA_*` state unexported, since only this shell reads it.
- **Global compinit.** While taking over, `.zshenv` sets `skip_global_compinit`
  to a marker value unless the user did. If the marker survives the user's
  `.zprofile`, the bootstrap `.zshrc` clears it and, with `GLOBAL_RCS` still on,
  repeats Ubuntu's own condition, so `compinit` runs once the user's `ZDOTDIR`
  is back.
- **New users.** The bootstrap `.zshenv` checks for the four startup files
  before sourcing the user's `.zshenv`, where zsh itself checks; `.zshrc` then
  runs `zsh-newuser-install`.
- **bash guests.** They drop these variables at their first `PROMPT_COMMAND`.
- **One bootstrap.** The file writer is shared with the local zsh integration,
  so one bootstrap remains the authority.

## Rejected alternatives

- `--exec zsh`: replaces the user's chosen login shell (the 1.1.0 regression).
- Write rc files into the guest through `wsl.exe`: an extra subprocess and
  guest mutation on every pane spawn, with a race against shell startup.
- A second WSL-specific zsh script: two diverging report implementations.
- Rewrite the files on every spawn: fsyncs on the UI thread and a replace that
  can race a guest zsh that is still sourcing them.
- Forward the zsh variables for every guest command and rely on the interactive
  gate in `.zshenv`: it does cover a zsh started later, but a command without
  zsh (an `sh -c` installer) keeps the host `ZDOTDIR` for good.
- Delete `.zcompdump` from the bootstrap after the global rc: the dump would be
  rebuilt on every start, which is slower than skipping and repeating compinit.
- Re-source `/etc/zsh/zshenv` to recover a conditional system `ZDOTDIR`: it
  runs the system file twice, duplicating whatever else it sets.
- fish through `XDG_CONFIG_HOME`/`XDG_DATA_DIRS`: replacing them changes where
  fish and other programs find all user and system data. fish is not handled.

## Consequences

- **Startup cost.** An interactive guest zsh reads three small files from the
  host drive at startup. A non-interactive zsh reads only `.zshenv` and then
  runs without the bootstrap.
- **Unreadable bootstrap.** If automount is disabled (`[automount] enabled=false`)
  or `/p` translation fails, zsh cannot read the bootstrap and also skips the
  user's startup files for that session. The same happens when drvfs is
  mounted with `metadata` and a restrictive `umask` and the pane uses `-u` for
  another guest user. UNC or redirected data directories do not reach this path.
- **Opt-out.** Adding `ZDOTDIR` to the host `WSLENV` (with or without a value)
  makes Pebrel leave zsh untouched; that is also the workaround for the
  unreadable cases.
- **Other login shells.** A fish or nushell login shell keeps the host `ZDOTDIR`
  for the whole session, because the host cannot see the guest's login shell. A
  zsh started from it still restores the user's files; a program that reads
  `${ZDOTDIR:-$HOME}` itself, such as a zsh framework installer, writes into the
  bootstrap and loses that write when the next process rewrites the files.
- **Nested zsh under a command.** A guest command such as `wsl bash`
  followed by `zsh` gets no zsh reports, since the command is not zsh.
- **Global compinit elsewhere.** Only Ubuntu's `skip_global_compinit` contract is
  repeated; another distribution whose global rc runs `compinit` unconditionally
  still writes its dump into the bootstrap.
- **System zshenv.** A guest `/etc/zsh/zshenv` that unconditionally assigns
  `ZDOTDIR` wins, and the integration becomes inert. One that assigns it only
  when it is unset sees the bootstrap value, so the user's files are read from
  `$HOME` instead.
- **Early bash processes.** Processes started by a bash guest's `.bashrc` before
  the first prompt still inherit `ZDOTDIR`.
- **Write failure.** A failed first write logs one warning and omits the
  variables for the rest of the process, restoring the previous behavior. A
  failed rewrite of a deleted file warns and omits them for that spawn.

## Validation

- **Rust unit tests.** `shell_detect` tests cover entry flags, preservation of
  existing `WSLENV` entries, a user-forwarded `ZDOTDIR` and guest commands that
  are not zsh. The bash
  `PROMPT_COMMAND` test checks that the variables are dropped. The
  `shell_integration` tests check that the files carry no CR, chain each user
  startup file, gate the takeover on interactivity, and require a fixed local drive.
- **Real zsh.** The Python suite runs the bootstrap in real zsh for interactive
  login and nested non-interactive shapes, with global rc files enabled on an
  Ubuntu host, with no user startup files, and for variables seen by the user's
  rc. Negative cases cover `GLOBAL_RCS` off, `skip_global_compinit` in
  `~/.zprofile` and an XDG `ZDOTDIR` set by `~/.zshenv`. It passed on the Ubuntu
  26.04 WSL guest; the leak, compinit, newuser and export cases fail against the
  first-cut scripts.
- **Not automated.** An end-to-end pane spawned through `wsl.exe`.

## Supersedes

None.

## Revisit when

WSL offers a supported guest-side startup hook, automount-disabled users report
lost configuration, or fish integration gains a non-invasive injection point.
