# Share shell discovery during startup and pane launch

## Status

Implemented.

## Context

Building a list of installed shells reads the Windows registry, the WSL
distribution list, and executable paths. Several startup checks used the same
snapshot, and a pane restoring a fixed shell still resolved the current default.

## Evidence

[#996](https://github.com/Kuddev/pebrel/pull/996) measured about 17 ms per discovery
pass on Windows 10 and found repeated passes during one startup. Lookup, restore,
and picker paths shared the same underlying discovery implementation.

## Decision

Use one process snapshot. The existing settings and shell selectors refresh on
open. An explicit shell-ID miss refreshes once before returning the unknown-ID
error; frozen local launches and SSH or serial panes do not resolve the default.
Startup-directory validation still resolves imported profiles to their actual
program because a profile ID does not identify a WSL launch.

## Rejected alternatives

Caching only in the old shell misses the shared launch paths. Classifying an
imported profile by ID can mistake a Linux guest directory for a host path.

## Consequences

Discovery remains synchronous when a selector refreshes. Repeated reads clone
the current entries without repeating the registry and filesystem scan.

## Validation

The cache and refresh regressions check discovery call counts. Explicit
unknown IDs continue to fail after one refresh. Imported profile resolution
keeps its existing startup-directory path.

## Supersedes

None.

## Revisit when

The operating system can notify the application when installed shells change.
