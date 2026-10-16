# Git changes hierarchy remains a presentation of one repository snapshot

## Status

Proposed for review.

## Context

Issue #538 requests an optional directory tree for changed files. The VCS drawer
already owns a virtual list with separate conflict, staged, and unstaged sections.
A file can appear in more than one section, and identical basenames are common.

## Evidence

- [`VcsList`](../../../../../../nebula_app/src/gpui_shell/workspace/vcs_panel/list_model.rs)
  consumes the existing `Arc<GitInfo>` and assigns each file its section, original
  source index, status, relative path, and operation kind.
- [`rows`](../../../../../../nebula_app/src/gpui_shell/workspace/vcs_panel/rows.rs)
  builds only descriptors requested by GPUI's virtual list. Git actions accept the
  captured repository-relative path, never a current visible index.
- Git activation currently opens the file or merge editor. It does not display
  an ordinary Git patch; that part of the issue needs a separate viewer boundary.

## Decision

Keep the existing snapshot as the sole VCS authority. Build directory descriptors
from its paths once when the snapshot or VCS view changes, without enumerating the
filesystem. Switching List/Tree and expanding a directory only selects a cached
projection. The default remains List. Layout and directory folds belong to this
workspace's VCS view and do not introduce persistent preferences.

Directory identity includes its section and exact relative path. File operations
retain the original section, source index, status and path. Scroll anchors use
section/path identity so inserting or sorting descriptors does not change the
visible file on refresh. Changing repositories resets folds and scrolling.

## Rejected alternatives

- Enumerating a filesystem tree during render would add blocking work and would
  disagree with staged deletions, rename destinations, WSL paths and snapshots.
- Replacing file identity with a flattened tree index could stage or open a
  different file after collapse or refresh.
- A new settings store or duplicate Git reader would add another authority for
  a view-only option.

## Consequences

The cached flat and hierarchical descriptors require memory proportional to the
snapshot paths and their directory components. Repainting only constructs visible
controls. Tree sorting is component-based, with directories before sibling files,
and does not interpret a backslash in a Git filename as a path separator.

## Validation

[`tree_tests`](../../../../../../nebula_app/src/gpui_shell/workspace/vcs_panel/tree_tests.rs)
protect section-specific folds, source operations, root and nested paths, rename
destinations, conflicts, Unicode, history/SVN boundaries, scroll identity and
repository changes. TestWindow coverage exercises the real layout controls,
directory keyboard activation, a thousand-file viewport and actual Git stage /
unstage against a temporary repository with repeated basenames. Physical visual
acceptance and ordinary Git patch display remain separate requirements.

## Supersedes

None.

## Revisit when

The product adds durable layout preferences, a Git patch viewer or a different
snapshot representation, or profiling shows snapshot projection exceeds the
drawer responsiveness budget.
