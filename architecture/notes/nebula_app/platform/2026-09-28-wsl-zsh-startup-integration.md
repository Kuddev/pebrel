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

Maintainer review of the submitted cut found two startup regressions that no
guest-side script can repair, because they happen before or without any zsh
reading the bootstrap:

- `ZDOTDIR` was replaced before the guest could verify that the bootstrap is
  reachable. With `[automount] enabled=false`, a failed `/p` translation or
  drvfs permissions for another `-u` user, zsh reads nothing there and, since
  it takes user files from `$ZDOTDIR` and falls back to `$HOME` only when it is
  unset, skips the user's own startup files too. A manual `WSLENV` opt-out
  after startup has broken is not a fallback.
- An unspecified guest command was taken as proof that the login shell is zsh.
  A fish or nushell login (and bash before its first prompt) received the host
  `ZDOTDIR`; nothing restores it there, and an installer using
  `${ZDOTDIR:-$HOME}` writes into the shared bootstrap.

## Decision

- **Host side.** For WSL spawns the host writes the same three files, CR-normalized,
  into `<data dir>/shell-integration/wsl-zsh`. It writes them once per process
  and only when that directory is on a fixed local drive. A failed first write
  is cached; a bootstrap file deleted while the process runs is rewritten.
  It then adds `ZDOTDIR/pu`, `NEBULA_ZSH_INTEGRATION/pu` and
  `NEBULA_ZDOTDIR_WAS_SET/u` to `WSLENV`.
- **Guest command.** A launch that runs a guest command other than zsh
  (`wsl htop`, `wsl sh -c …`, `-e bash`) receives none of the zsh variables and
  is never probed: no zsh would take them back, and an installer that finds
  `ZDOTDIR` would write its `.zshrc` into the host bootstrap.
- **The guest answers, not the host.** Before the first takeover of a
  (distribution, user), `platform::wsl_guest_shell` runs one
  `wsl.exe --distribution <d> [--user <u>] --exec sh -s` with
  `res/shell/wsl-guest-probe.sh` on stdin and the bootstrap sent through the
  same `WSLENV` `/p` translation the pane will use. The guest reports the login
  shell from its passwd entry and whether that user can read all three
  bootstrap files there. The verdict is cached per process; a failed probe is
  retried after five minutes. A login shell takes the bootstrap only when the
  guest said zsh **and** readable, an explicit `-e zsh` only when readable.
  Unknown (failed, timed out, or `--distribution-id`/`--system` with no name
  to enter) leaves the guest environment untouched.
- **Bounded wait.** The spawn runs on the UI thread and waits at most two
  seconds for a guest's first verdict. Waiting costs no prompt time (the pane's
  login shell waits for the same guest start), but a guest still booting
  starts that pane without zsh reports; later panes use the cached verdict.
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
- Treat an unspecified guest command as zsh, or hand out `ZDOTDIR` without a
  reachability check (the submitted cut): the two regressions above. No
  guest-side script can recover from them, because zsh reads nothing from an
  unreadable `ZDOTDIR`, and fish/nushell never read the bootstrap at all.
- Read `\\wsl.localhost\<distro>\etc\passwd` and `wsl.conf` from the host
  instead of probing: it also starts the distribution and blocks, but cannot
  see a failed `/p` translation or another user's drvfs permissions.
- Probe without waiting: the first WSL pane of every process (every restored
  session) would never report zsh cwd. Wait without a bound: a broken WSL
  service would freeze the UI for the whole probe budget.
- Delete `.zcompdump` from the bootstrap after the global rc: the dump would be
  rebuilt on every start, which is slower than skipping and repeating compinit.
- Re-source `/etc/zsh/zshenv` to recover a conditional system `ZDOTDIR`: it
  runs the system file twice, duplicating whatever else it sets.
- fish through `XDG_CONFIG_HOME`/`XDG_DATA_DIRS`: replacing them changes where
  fish and other programs find all user and system data. fish is not handled.

## Consequences

- **Startup cost.** An interactive guest zsh reads three small files from the
  host drive; a non-interactive one reads only `.zshenv`. The first zsh pane of
  each (distribution, user) per process also pays one guest `sh` round trip,
  with the UI waiting up to two seconds; a cold distribution may not answer in
  time, and that pane then has no zsh reports.
- **Unreadable bootstrap.** If automount is disabled (`[automount] enabled=false`),
  `/p` translation fails, or drvfs permissions hide the files from the pane's
  `-u` user, the guest reports the bootstrap unreadable and the pane starts with
  its own startup environment: no zsh reports, no lost startup files. UNC or
  redirected data directories are withheld on the host before any probe.
- **Opt-out.** Adding `ZDOTDIR` to the host `WSLENV` (with or without a value)
  makes Pebrel leave zsh untouched.
- **Other login shells.** A fish, nushell or bash login receives no zsh
  variables, because the guest named it; only a passwd entry saying zsh, or an
  explicit `-e zsh`, is taken over. A zsh started from another shell is not.
- **Probe failures.** A guest that cannot run `sh` (no `/bin/sh`, a broken
  distribution) or does not answer within the budget is left alone and retried
  after five minutes; each failure logs one warning.
- **Nested zsh under a command.** A guest command such as `wsl bash`
  followed by `zsh` gets no zsh reports, since the command is not zsh.
- **Global compinit elsewhere.** Only Ubuntu's `skip_global_compinit` contract is
  repeated; another distribution whose global rc runs `compinit` unconditionally
  still writes its dump into the bootstrap.
- **System zshenv.** A guest `/etc/zsh/zshenv` that unconditionally assigns
  `ZDOTDIR` wins, and the integration becomes inert. One that assigns it only
  when it is unset sees the bootstrap value, so the user's files are read from
  `$HOME` instead.
- **Stale verdict.** `chsh` in the guest, or a changed mount, is seen by the
  next Pebrel process. A bash login receives no `ZDOTDIR` any more; its
  first-prompt cleanup stays as the guard for a passwd entry changed since.
- **Write failure.** A failed first write logs one warning and omits the
  variables for the rest of the process, restoring the previous behavior. A
  failed rewrite of a deleted file warns and omits them for that spawn.

## Validation

- **Rust unit tests.** `shell_detect` tests cover entry flags, preservation of
  existing `WSLENV` entries, a user-forwarded `ZDOTDIR` and reading the guest
  command from WSL's option region only. `platform::wsl_guest_shell` tests
  cover the probe command (distribution, user, `--exec sh -s`, the `/p`
  forwarded bootstrap), the answer parser, and the takeover decision with a
  guest double: non-zsh guest commands are never probed, `-e zsh` still needs a
  readable bootstrap, and a login shell is taken over only for zsh with a
  readable bootstrap (fish, nushell, bash, an empty answer and no answer all
  decline). The bash `PROMPT_COMMAND` test checks that the variables are
  dropped. The `shell_integration` tests check that the files carry no CR,
  chain each user startup file, gate the takeover on interactivity, and require
  a fixed local drive.
- **Real zsh.** The Python suite runs the bootstrap in real zsh for interactive
  login and nested non-interactive shapes, with global rc files enabled on an
  Ubuntu host, with no user startup files, and for variables seen by the user's
  rc. Negative cases cover `GLOBAL_RCS` off, `skip_global_compinit` in
  `~/.zprofile` and an XDG `ZDOTDIR` set by `~/.zshenv`. It passed on the Ubuntu
  26.04 WSL guest; the leak, compinit, newuser and export cases fail against the
  first-cut scripts. The probe script runs under real `sh` with a readable
  bootstrap, a bootstrap missing one file, a missing directory, an untranslated
  Windows path and an empty variable, and its shell answer is compared with the
  passwd entry rather than `$SHELL`.
- **Not automated.** An end-to-end pane spawned through `wsl.exe`; the probe
  command was run by hand against a real guest (see the PR evidence).

## Supersedes

None.

## Revisit when

WSL offers a supported guest-side startup hook, the two-second spawn wait shows
up in profiles or reports, or fish integration gains a non-invasive injection
point.
