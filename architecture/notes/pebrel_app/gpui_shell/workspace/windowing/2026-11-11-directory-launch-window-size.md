# Directory launches retain their normal window size

## Status

Proposed for [#922](https://github.com/Kuddev/pebrel/issues/922).

## Context

Explorer opens Pebrel with an explicit working directory. A new regular window
should use the last normal size and open a terminal in that directory.

## Evidence

Both cold startup and the resident runtime fallback choose
`WorkspaceStartup::NewTerminal` for this request. The geometry loader accepted
only `RestoreOrDefault`, so the first window in either path used the default
grid. A resident process allocates a new window ID after the last window closes.

## Decision

When an explicit directory opens the only regular window on Windows, load the
saved normal width and height through the existing session geometry reader.
Scope the values to the allocated window ID, which also tells the workspace to
size its initial terminal grid from the window. Explicit grid configuration
still takes precedence. An isolated process cannot use ordinary geometry.

## Rejected alternatives

- Use `RestoreOrDefault` for Explorer launches: restoring old tabs can override
  the requested directory and start programs the user did not ask to reopen.
- Extend the first-window ID check: that ID is already consumed when a
  resident process creates a replacement window.
- Persist another size setting: the ordinary session snapshot already stores
  the normal bounds at close.

## Consequences

Directory launches restore size while using ordinary placement for a new
window. Opening a second regular window retains the existing defaults.

## Validation

`directory_launch_uses_saved_size_without_restoring_the_old_window` exercises
both startup constructors, the geometry loader, and window options with a
saved maximized rectangle. The existing explicit-grid regression covers
configured dimensions.

## Supersedes

The Windows directory-launch eligibility rule in
[Ordinary-window startup geometry](../2026-10-03-ordinary-window-geometry.md).

## Revisit when

Regular geometry has a persistence owner independent of the session snapshot.
