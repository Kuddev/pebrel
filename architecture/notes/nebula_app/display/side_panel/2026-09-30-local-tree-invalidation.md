# Local file-tree invalidation

## Status

Proposed for repository review, 2026-09-30. No release claim.

## Context

The Files drawer rebuilt its tree only on navigation, explicit refresh or VCS
operations. A `git clone` into an already open directory therefore remained
invisible until the user navigated away and back. Filename search already had
notifications, but its on-demand cache did not own ordinary tree snapshots.

## Evidence

`SidePanel::sync_at` deliberately removed unconditional periodic scans: those
scans also launched VCS and WSL subprocesses, including expensive cold starts.
`EmbeddedFileIndex` only walks and installs watches when a search is requested.
`SidePanel::rebuild_rows` could also be overwritten by a snapshot started before
the user expanded or collapsed a directory.

## Decision

- Keep invalidation with the shared `SidePanel` model, not either renderer.
  Reuse the existing `notify` dependency and snapshot worker serialization.
- A lazily created, panel-owned worker installs non-recursive native watches for
  the local root and expanded directories present in the bounded tree snapshot.
  The root's parent is watched only to observe root replacement; unrelated
  siblings do not invalidate the tree. No recursive repository scan is added.
- Resolve backend path spellings on the worker (FSEvents uses canonical paths),
  retaining the root's directory entry to observe symlink-root replacement.
- Watch installation and release happen off the UI thread. A single-slot mailbox
  retains only the latest scope; callbacks retain atomic dirty/rearm flags, not
  an event queue. Each scope has a new signal so obsolete callbacks cannot dirty
  a new scope. Closing the drawer, selecting Git, entering the SSH browser or
  destroying the panel releases the worker. WSL guest paths are never watched as
  host paths.
- Watch installation requests one catch-up snapshot, closing the interval
  between the first enumeration and watch readiness. Root/directory replacement,
  overflow and watcher errors request rearming. Manual refresh retries watches.
- Native events update only tree rows, using the existing row/ignore annotation
  implementation, without refreshing the VCS snapshot or rebuilding filename
  search. Snapshot revisions reject obsolete refresh/root results; a separate
  tree revision rejects rows predating an expansion/collapse without losing the
  same root's still-valid VCS result.
  Expansion, selection, query and scroll state are not reset by notifications.
- Refresh requests are rate-limited to one per 250 ms, not delayed until all
  events stop. Events arriving during an active snapshot remain pending. Idle
  UI polling reads flags but never starts an unconditional filesystem scan.
- Bound native registrations to 1,024 and directory spellings to 256 KiB per
  scope representation. Existing row/entry limits still apply. Inaccessible paths, unsupported
  notification backends and paths beyond watch limits retain manual refresh;
  this is not a polling fallback or a remote filesystem watcher.

## Rejected alternatives

- Restoring a timer-driven whole snapshot would repeat the original WSL/VCS
  subprocess cost even while idle.
- Recursive watching of every clone/build dependency would give closed subtrees
  resource ownership unrelated to what is visible.
- Making the filename-search worker responsible would couple tree liveness to
  query cancellation, global search work serialization and cache eviction.
- Watching directly on the UI thread would make registration/cleanup latency
  part of rendering. An unbounded event channel would retain clone storms.

## Consequences

An open local Files drawer owns an additional sleeping worker and native watch
resources. The root and visible expanded scopes may briefly require a second
catch-up snapshot when their watches change. Automatic snapshots still use the
existing Git ignore annotation; they do not run the full VCS status/history read.
Existing explicit navigation/expansion I/O is not redesigned here. SSH and WSL
automatic refresh remain separate capabilities, not implied by local watching.

## Validation

Production-source regressions cover native local clone, create/rename/delete,
expanded/collapsed directories, root replacement, search isolation, stale
snapshots, scope cleanup, WSL exclusion, in-flight events and bounded bursts.
Idle-sync checks assert that no snapshot revision changes without invalidation.
Report executed platform/product checks separately in the PR; model tests are
not a claim of manual GUI or cross-platform visual acceptance.

## Supersedes

None. This preserves the no-periodic-rescan decision while removing its stale
local-tree consequence.

## Revisit when

Native registration cost, unsupported mounts or the watch bounds cause a
demonstrated workflow problem, or a remote event stream supplies an independent
WSL/SSH invalidation source. Preserve bounded ownership and idle behavior.
