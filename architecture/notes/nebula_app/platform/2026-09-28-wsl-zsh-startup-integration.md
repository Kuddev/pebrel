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

## Decision

- **Host side.** For WSL spawns the host writes the same three files, CR-normalized,
  into `<data dir>/shell-integration/wsl-zsh`. It writes them once per process
  and only when that directory is on a fixed local drive. It then adds
  `ZDOTDIR/pu`, `NEBULA_ZSH_INTEGRATION/pu` and `NEBULA_ZDOTDIR_WAS_SET/u` to
  `WSLENV`.
- **Unknown guest `ZDOTDIR`.** The host cannot observe the guest's original
  `ZDOTDIR`, so it is treated as unset.
- **User-forwarded `ZDOTDIR`.** If the host already forwards one, Pebrel leaves
  zsh untouched.
- **Takeover scope.** The bootstrap `.zshenv` takes over only an interactive
  shell (`-o rcs && -o interactive`). A non-interactive zsh restores the user's
  `ZDOTDIR` and unsets the `NEBULA_*` variables. When taking over, it exports
  `NEBULA_ZDOTDIR_WAS_SET` and `NEBULA_ORIGINAL_ZDOTDIR` together.
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
- Skip the takeover on the host when wsl arguments carry a command: the
  interactive gate in `.zshenv` also covers commands that start `zsh` later.
- fish through `XDG_CONFIG_HOME`/`XDG_DATA_DIRS`: replacing them changes where
  fish and other programs find all user and system data. fish is not handled.

## Consequences

- **Startup cost.** An interactive guest zsh reads three small files from the
  host drive at startup. A non-interactive zsh reads only `.zshenv` and then
  runs without the bootstrap.
- **Unreadable bootstrap.** If automount is disabled (`[automount] enabled=false`)
  or `/p` translation fails, zsh cannot read the bootstrap and also skips the
  user's startup files for that session. UNC or redirected data directories do
  not reach this path.
- **System zshenv.** A guest `/etc/zsh/zshenv` that unconditionally assigns
  `ZDOTDIR` wins, and the integration becomes inert. One that assigns it only
  when it is unset sees the bootstrap value, so the user's files are read from
  `$HOME` instead.
- **Early bash processes.** Processes started by a bash guest's `.bashrc` before
  the first prompt still inherit `ZDOTDIR`.
- **Write failure.** A failed host write logs a warning and omits the variables,
  restoring the previous behavior; the next spawn retries.

## Validation

- **Rust unit tests.** `shell_detect` tests cover entry flags, preservation of
  existing `WSLENV` entries and a user-forwarded `ZDOTDIR`. The bash
  `PROMPT_COMMAND` test checks that the variables are dropped. The
  `shell_integration` tests check that the files carry no CR, chain each user
  startup file, gate the takeover on interactivity, and require a fixed local drive.
- **Real zsh.** The Python suite runs the bootstrap in real zsh for interactive
  login and nested non-interactive shapes. It passed on the Ubuntu 26.04 WSL
  guest, and the leak case fails against the previous `.zshenv`.
- **Not automated.** An end-to-end pane spawned through `wsl.exe`.

## Supersedes

None.

## Revisit when

WSL offers a supported guest-side startup hook, automount-disabled users report
lost configuration, or fish integration gains a non-invasive injection point.
