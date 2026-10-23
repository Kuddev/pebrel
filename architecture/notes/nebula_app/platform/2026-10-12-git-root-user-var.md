# The `pebrel_git_root` UserVar protocol (remote repository root reporting)

## Status

Accepted 2026-10-12.

## Context

Sidebar SSH host grouping needs the remote "project/directory" identity;
matching the local grouping logic requires the **repository root**
(sibling directories of one repository group together). Remote
cwd/branch already have channels (OSC 7 plus the `NEBULA|cwd|branch`
title); the repository root had none.

## Evidence

- The `res/shell/bashrc`/`zshrc` precmd already runs one
  `git rev-parse --abbrev-ref HEAD` per prompt for the branch chip (one
  fork per prompt); the root is available from the same call
  (`--show-toplevel`).
- `nebula_terminal/src/osc_cwd.rs::parse_user_var` already understands
  `SetUserVar=<name>=<base64>`: name whitelist `[A-Za-z0-9_]`, UTF-8, an
  8 KiB cap; unknown names are harmlessly ignored by older clients and
  keep line ordering (UserVar events never interleave with data in the
  event loop).

## Decision

The repository root rides **OSC 1337
`SetUserVar=pebrel_git_root=<base64>`**:

1. The precmd merges branch and root into one
   `git rev-parse --abbrev-ref HEAD --show-toplevel` when
   `PEBREL_PROJECT_REPORT=1`. This is an added prompt subprocess; disabled
   sessions do not launch it. The explicit flag is captured at local/WSL spawn
   or SSH bootstrap, overriding inherited state; the `NEBULA|cwd|branch` title shape is untouched.
2. Emitted only on the root's **change edge** (once entering and once
   leaving a repository; the base64 fork happens only on the jump, zero
   extra forks per prompt otherwise). Leaving a repository emits an
   empty value and the receiver clears its state.
3. Bare repositories print only the branch line: a single-line output
   means "no worktree root".
4. The receiving side keeps `TerminalView::git_root: String` (empty =
   none) in the `UserVar` arm and emits `TitleChanged` on value changes,
   feeding `resolve_project_group` from the workspace TitleChanged event. The
   rc files are shared by local and remote panes
   (`platform/shell_integration.rs` and the SSH integration include the
   same copy): local panes store but never consume it — consumption is
   strictly gated on `ssh_destination.is_some()`.

## Rejected alternatives

- A fifth `NEBULA|` title field: older clients `splitn(3)` would fold
  the fifth segment of `NEBULA|cwd|branch||root` into the program slot
  → `running_program = Some("|root")` → `program_is_work` always true →
  that remote pane stuck **Running forever** — the exact failure shape
  the remote status work set out to eliminate. It would fire precisely
  under version skew (a new client connecting to a shared remote whose
  rc an old client has not yet re-pushed). Rejected.
- Probing over the SSH exec channel: one round trip per cd is
  unacceptable on slow links.

## Consequences

- New client + old rc (first connection after an upgrade, before the
  re-push): the var never arrives, subgroups fall back to cwd/fallback,
  and one reconnect upgrades the integration automatically (the
  manifest asset hash only blocks user-modified files; our own older
  copies can be overwritten).
- Old client + new rc: the var is harmlessly swallowed by the old
  parser, the title shape is unchanged, zero pollution.

The UI switch takes effect immediately. Existing shell processes keep their
launch environment; reconnect/open a new terminal to change project reporting.
Local project discovery is a coalesced background job, never UI-thread file I/O.

## Validation

The existing shell integration suite includes explicit opt-in/out cases using a
fake git function: disabled prompts do not invoke git, enabled prompts do, and
returning the flag to zero stops further calls. Windows skips the Unix PTY suite;
Bash syntax is checked locally and native Linux/macOS CI runs the behavioral cases.

## Supersedes

None.

## Revisit when

- The title protocol is upgraded wholesale (e.g. to a structured OSC):
  fold the root back into the same channel.
