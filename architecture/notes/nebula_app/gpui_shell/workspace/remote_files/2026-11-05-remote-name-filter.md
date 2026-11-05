# Remote SFTP name filter over the current listing

## Status

Proposed with the SFTP sidebar name-filter update.

## Context

The local file drawer has a filter box with case, whole-word and regex toggles.
The remote SFTP drawer lists one directory at a time and had no way to narrow
it, so large directories could only be searched by scrolling.

## Evidence

`SftpBrowseSession::list_dir` already returns the complete entry list for one
directory, and `remote_rows` renders exactly that list plus the synthetic parent
row. The local matcher already owns the meaning of case folding, whole-word and
bounded regex compilation in `display/side_panel/search/matching.rs`; a second
implementation would drift from it.

## Decision

The filter matches only entries of the directory already on screen; files and
directories share one name-only rule (`NameMatcher`, backed by the local
`QueryMatcher`). Compilation happens when the query or a toggle changes, never
per frame. Selection is retained only for visible rows, and the empty notice
distinguishes "no match" from "directory is empty" and "query failed".

The query is cleared when the pane or directory changes, because the input
entity is shared by all panes and `bind`/`navigate_remote` run without a window;
replaying a per-pane query would need another snapshot field and a deferred
focus restore. Refreshing the directory on screen keeps the query, matching the
selection retention that refresh already has. Escape in the list clears the
filter before the selection.

## Rejected alternatives

- Walking the remote tree recursively would add network requests, cancellation,
  generation tracking and result limits for a box that filters one listing.
- A second matcher in `remote_files.rs` would fork case folding and regex limits.
- Restoring a per-pane query needs an input-value snapshot plus focus handling
  that the drawer's pane switching does not otherwise keep.
- Highlighting matched substrings is a separate render change; plain filtering
  keeps the row layout shared with the unfiltered list.

## Consequences

No network, protocol, persistence or worker changes. Filtering allocates one
entry vector per render, bounded by the directory already in memory. The input
entity is created on the first remote render and its subscription is stored in
`RemoteBrowser`, so dropping the browser detaches it. Switching panes loses the
filter deliberately; the listing, selection and scroll keep their existing
per-pane snapshots.

## Validation

Unit tests cover default case-insensitive matching, the three toggles, invalid
regex errors and clear/reset behavior. A GPUI regression with the in-memory SFTP
peer covers filtered rows, the retained parent row, selection retention, the
query surviving a refresh, escape and the reset on directory change. Formatting,
architecture checks and native tests run in GitHub Actions; no Rust toolchain
was available locally.

## Supersedes

None.

## Revisit when

The browser can search across directories, or users need the query to survive a
pane switch.
