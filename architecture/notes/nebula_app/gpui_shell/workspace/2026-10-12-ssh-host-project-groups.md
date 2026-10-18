# SSH host two-level grouping in the unified sidebar row model

## Status

Accepted 2026-10-12.

## Context

The sidebar's "Current tasks" section previously flattened every SSH pane
into one first-level group keyed by the destination string
(`tab_group_info` set `key = ssh_destination` directly): tasks from
different projects on the same host mixed together, and there was no
"host" layer of identity at all. The local pane grouping (git root first,
otherwise cwd, with a "Workspace" fallback) could not be reused for
remote panes.

## Evidence

- `task_groups.rs` `tab_group_info`/`grouped_rows` (the pre-change SSH
  branch) shows the flat keying.
- Remote cwd/branch already arrive via OSC 7 and the `NEBULA|` title
  (`res/shell/bashrc`/`zshrc` precmd); the remote repository root is
  reported via the `pebrel_git_root` UserVar (see
  [`2026-10-12-git-root-user-var.md`](../../platform/2026-10-12-git-root-user-var.md)).

## Decision

1. `SidebarRow` gains `HostHeader` (the host-level container, key
   `ssh:{destination}`); project subgroups and their entries carry
   `depth` (local 0 / SSH 1); `TabGroupInfo` gains `host`.
2. Key namespaces: host key `ssh:{dest}`, child key
   `ssh:{dest}|{git_root|cwd|workspace}`. Collapse state and first-open
   ordering share the same tables as local groups and `recent:` keys,
   separated by prefix.
3. Remote group keys deliberately stay **out** of
   `project_group_cache`: a remote `/home/x` and a local `/home/x` are
   the same string and would collide, and the remote git root arrives
   directly from the integration — it needs (and cannot use) a local
   stat probe. Derivation lives in `task_groups_ssh::ssh_group_info`,
   shared by the event-path bookkeeping and render so they cannot drift.
4. Top-level ordering: SSH hosts and local groups interleave by
   first-open order with the local fallback group globally last; inside
   a host, project subgroups order by first open with the host fallback
   subgroup last. Unbookmarked containers fall back to first-seen order.
5. Tree guides: the host header is a pure container row and draws **no
   host-level guide** (`hostv`/`hoste` were implemented and removed by
   user decision 2026-10-13, see Rejected); subgroups and their entries
   are **not indented** (same-day second ruling, the depth mechanism was
   deleted wholesale) and share the same spine x as local groups, whose
   existing guide geometry is unchanged point for point.
6. Host headers are named "SSH {destination}": typed placeholder message
   `sidebar.ssh_host` (en-US/zh-CN, other languages fall back to
   English), same precedent as `settings.network.route.ssh_jump`
   ("SSH {target}"); SSH is a technical abbreviation identical across
   languages.
7. "New terminal" from a focused SSH task (sidebar "+",
   Ctrl+Shift+T, and the panel NewTab all funnel through
   `add_terminal`) inherits the same host plus the current remote task
   directory: the pure decision function `focused_ssh_origin` feeds the
   existing `add_ssh_terminal_at`. The settings "startup directory" is
   a local path and does not apply to SSH tasks, so the SSH branch
   takes precedence. When the cwd has not been reported the host is
   still inherited and the directory falls back to the remote default.

## Rejected alternatives

- Child keys from the remote cwd alone (no host layer): sibling
  directories of one repository would not group, diverging from the
  local semantics; and the "host" container is what users expect.
- Host-level tree guides (`hostv` vertical + `hoste` horizontal arm to
  the subgroup header center): the `hoste` endpoint lands exactly on the
  subgroup's collapse-arrow slot, with the 2 px segment sitting on the
  arrow (removed after the user reported the occlusion); host identity
  is carried by the "SSH {destination}" naming instead.
- One indent step for subgroups (a `depth: u8` carried on rows was
  implemented): with host guides removed the indent lost its alignment
  anchor and only wasted horizontal space (ruled out the same day); the
  depth mechanism was deleted wholesale rather than kept as an
  always-zero field.
- Probing the remote git root over the SSH exec channel
  (`git rev-parse --show-toplevel`): one remote round trip per cd is
  unacceptable on slow links; the precmd already runs one rev-parse for
  the branch, so taking the root from the same command is zero
  incremental cost.
- Remote keys inside `project_group_cache` (cwd→key): local/remote path
  string collisions.

## Consequences

- Typed `ssh` (a local pane where the user typed `ssh`, the Windows
  wrapper) has no `ssh_destination` and does not join host grouping —
  the requirement is scoped to native SSH tasks; known boundary.
- Without remote integration (no python3 etc.) the cwd is unknown and
  entries land in the host's "Workspace" fallback subgroup, matching the
  local degradation shape when cwd has not been reported.
- `sidebar_group_collapsed` is in-memory state; key scheme changes carry
  no migration cost.

## Validation

- `task_groups` grouping/ordering/collapse/guide unit tests, including
  `ssh_tabs_nest_under_one_host_header_with_project_subgroups`,
  `hosts_mix_with_local_groups_by_first_open_and_fallbacks_sink`,
  `tree_guides_skip_host_headers_but_keep_nested_project_elbows`.
- `task_groups_ssh` key-derivation unit tests.
- New-terminal inheritance: `new_terminal_from_an_ssh_task_inherits_host_and_remote_cwd`
  (pure decision function; `add_ssh_terminal_at` writes real
  `saved_hosts`, so the end-to-end path is not exercised in unit
  tests).

## Supersedes

None.

## Revisit when

- The remote integration starts reporting finer project identity (e.g.
  worktrees), or the "Recent tasks" section needs the same host
  hierarchy.
