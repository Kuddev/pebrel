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

## Decision

`shell_detect::wsl_spawn_distro` returns the explicit distribution or the
registry default. It is called once per spawn, and completion scoping reuses
the same value. `--distribution-id` resolves to `None` rather than to the
default. `TerminalView` snapshots the value as `wsl_distro`. The workspace WSL
location and prompt-path links read the focused pane's snapshot instead of the
tab launch. `wsl_launch_distro` keeps its explicit-only semantics for
launch-argument rewriting. `is_wsl_launcher` is the single WSL program detector
for the snapshot, the cwd report environment and argument rewriting.

Copies of a pane follow its snapshot:

- **Split and duplicate** insert `-d <snapshot>` into a bare launch, after a
  leading `~`. A later default change therefore cannot move the copy to another
  guest.
- **A WSL pane without a guest cwd** (fish, or before the first prompt) splits
  into the same guest rather than the host default shell.
- **A new default-shell tab** inherits the guest cwd only when it targets the
  same distribution and explicit user, and does not choose its own directory.
  Otherwise it receives only a `/mnt/<drive>` host directory; a UNC probe would
  block the UI thread.

## Rejected alternatives

- Read the registry default whenever a location is needed: a later default
  change would silently retarget a running pane — the guess the old rule forbade.
- Rewrite bare launches to `-d <default>`: changes persisted launch identity and
  restore semantics for users who intentionally follow the default.
- Ask the guest for `WSL_DISTRO_NAME` via OSC: needs a new protocol and does not
  cover shells without the report integration.
- Drop the guest command (`-e htop`) when splitting: it would also drop
  shell-selecting commands such as `--exec zsh -l`, so splits keep the pane's
  command as duplicates do.

## Consequences

A default change between the registry read and `wsl.exe` start could still
mismatch; the window is the same spawn call. Restored panes resolve again at
their own spawn. SSH and host panes have no snapshot. A `--distribution-id` pane
has no WSL location until the id is mapped to a name. Splitting a pane whose
launch runs a guest command runs that command again.

## Validation

- **Registry-free unit tests.** Explicit, bare, id-based and non-WSL
  resolution, with an injected default.
- **Launch rewriting.** Pinning and the `~` marker.
- **Pane snapshot.** Prompt-path mapping from the snapshot.
- **Split and new-tab launches.** Guest cwd, no guest cwd, another user and a
  profile directory.
- **Not automated.** Interactive file-tree following.

## Supersedes

None. Narrows the explicit-only rule documented at `wsl_launch_distro`.

## Revisit when

WSL exposes the running distribution of a process, or panes gain an OSC-reported
guest identity.
