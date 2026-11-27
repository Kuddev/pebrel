# Background ignore annotations and incremental tree toggles

## Status

Implemented, pending review.

## Context

The Files drawer flattened the whole expanded tree whenever the user toggled a
directory, and the flatten ran `git check-ignore` synchronously at its end.
`click_row` therefore walked every expanded directory and spawned git on the UI
thread on every expand/collapse, even though a comment claimed "no git re-run".
The same synchronous annotation also delayed the first rows of an ordinary
background refresh, because the worker published nothing until the whole rows
vector, ignore marks included, was complete.

## Evidence

On the reference machine one `git check-ignore -z --stdin` over 500 paths took
101-109 ms and over 1000 paths took 309-359 ms. The tree cap is
`MAX_ROWS = 1000`, so the slow case was reachable with a normal project. The
initial `refresh` worker already published rows and VCS state in two stages,
but `tree_rows` still called `SidePanel::mark_ignored` before the first
publication, and `click_row -> rebuild_rows -> tree_rows` re-ran the complete
flatten plus git on the interaction path.

## Decision

`tree_rows`/`tree_rows_wsl` now only enumerate. The snapshot worker publishes
two stages: (1) tree rows with no ignore annotations, then (2) the same rows
annotated by git plus the VCS snapshot. A full refresh therefore paints the
directory listing without waiting for ignore or VCS probing. The first stage
carries the ignore marks of the rows already on screen, matched by absolute
path, so refreshing an annotated tree does not blank every italic mark for a
frame; rows that appear for the first time stay unannotated until stage two,
which is authoritative and can also clear a stale mark.

`click_row` is incremental. Collapse removes the contiguous DFS descendant run
from the in-memory rows and reads nothing. Expand reads only the newly opened
directory (plus descendants that are already in `expanded` but were hidden by
a collapsed ancestor) and inserts those rows; the global `MAX_ROWS`,
`MAX_PER_DIR`, ordering and `.git` filtering rules are unchanged.

An incremental expand is also bounded by the row cap: it reads only the rows
that still fit after the clicked directory (`MAX_ROWS - index - 1`), because
anything past that is discarded by the `truncate` that keeps the incremental
result identical to a full re-flatten. Whenever the applied row set fills the
cap — a full snapshot or an inserted subtree — it is recorded as truncated,
because rows may exist past the cut. Collapsing a subtree in that state re-arms
a snapshot: the freed space is not filled by the rows the cap dropped, and only
a re-flatten can bring them back. A host expand whose `read_dir` fails (the
directory was deleted between snapshots) drops the path from `expanded` and
re-arms the snapshot instead of leaving an open chevron over nothing.

New rows start unannotated. The expand queues their absolute paths in
`pending_ignore_paths`; the next idle `sync` hands exactly those paths to a
background pass that runs one `git check-ignore` over them, so a click never
re-annotates the whole tree. That pass publishes an ignore-only update keyed by
absolute path and rewrites only the rows it was asked about, so rows already on
screen keep their mark and a concurrent expand/collapse is never overwritten. It
reuses the existing single snapshot worker and slot: if a worker is already in
flight the request stays pending and the queued paths coalesce into one
follow-up pass.

Full snapshots also carry `tree_generation`, the identity of the visible tree
shape. Root changes and every expand/collapse bump it; a filter change
deliberately does not, because the filter only selects among `rows` and never
changes the row set, and a bump there would discard a running full scan and pay
for another one. A row-set publication is applied only when its generation
still matches; an out-of-date row set is dropped instead of reverting the
incremental change, and its VCS result is still consumed. A dropped row set
re-arms the next snapshot (`needs_refresh`), because the `refresh` that
produced it already consumed its request: the files it read and the ignore
annotations it computed must not be lost just because the tree shape changed
while it ran. That re-scan is paid only when a publication was actually
discarded — a toggle by itself still never re-walks the tree. Ignore-only
updates are path-keyed and remain safe to apply across generations.

`ignored_paths` first checks `git_repository_root`; directories with no `.git`
ancestor never spawn git. That check lives in the worker, not on the UI thread:
the pending flag is polled every 100 ms, and repeating an ancestor walk there
would duplicate the worker's own check. WSL trees remain unannotated, matching
the previous behavior where only host `tree_rows` was marked.

## Rejected alternatives

- Keeping the synchronous re-flatten and only moving git to a thread: the UI
  thread would still walk every expanded directory per click.
- Scheduling a full background refresh per toggle: it restores the ignore marks
  but re-walks the tree and re-runs VCS probes for a purely local shape change.
- Applying a background ignore result as a whole rows vector: a later
  expand/collapse could be overwritten by rows captured before the click.
- Spawning git in directories outside a repository to get a definitive "not
  ignored": git can only fail there, at process-spawn cost.

## Consequences

Ignore marks appear after the panel's existing 100 ms poll rather than in the
same frame as the newly opened rows, and host rows never block on git. A full
refresh keeps the previous marks until the annotated stage lands, so the only
visible cost is that a mark this snapshot would remove (or one a brand-new row
needs) changes one poll later. WSL expansion still performs one blocking guest
`find`, but it now covers only the newly opened directory (plus hidden expanded
descendants) instead of every expanded directory. Root/`files_wsl` staleness
handling is unchanged; the ignore-only update is additionally path-keyed, so it
cannot revert concurrent tree edits. A failed WSL expansion keeps the visible
rows and re-arms the regular snapshot path so the existing read-failure state
still surfaces; a failed host expansion does the same and drops the stale path
from `expanded`. Collapsing a row set that the cap truncated — a snapshot or
an expand — re-arms one full snapshot instead of leaving the tree permanently
short.

## Validation

Unit tests cover the two-stage publication order (rows without marks first,
annotated rows second), that the bare stage keeps the previous marks while the
annotated stage stays authoritative, the no-`.git` guard, the pure
collapse/expand row helpers, the cap-truncation flag, the failed host expansion
and the rule that neither a stale row set nor a filter change can discard an
expansion. The existing side-panel tests continue to exercise ordering,
expansion and snapshot staleness. `scripts/check_architecture.py --base
origin/main` passes, and the changed files are rustfmt-clean.

## Supersedes

None.

## Revisit when

The drawer gains a persistent filesystem watcher that can invalidate ignore
annotations, or ignore state must be exact within a single frame of expansion.
