# Automatic SSH host icons

## Status

Implemented for the desktop test candidate.

## Context

The automatic icon setting fell back to a terminal glyph even after a connection had identified the host. A previous prototype recorded the reported system in a separate hint file (commit `5c4dbce8`).

## Evidence

The host list and launcher resolve icon IDs through `SshProfiles::icons`. The settings pane persists across openings, so it needs to refresh hints without replacing edits in the host library.

## Decision

For an automatic icon, the interactive SSH session opens one optional exec channel after the shell is ready. The probe has a six-second deadline and an 8 KiB output limit. It reads the kernel name and distribution data on the authenticated connection. The SSH pane owns the probe; it stops waiting for it when the pane closes.

Send the saved choice or hint as the connection resolves, then send a recognized system to the same pane over its SSH presentation channel. The focused pane supplies the icon in both tab layouts; an active Agent retains its existing brand icon. Store the last recognized icon in a bounded map beside the profiles. A manual choice always wins, and the settings pane reloads only the hints on its background executor when it reopens.

## Rejected alternatives

- Put detected values in the profile. A connection would compete with the editor and turn a hint into a user choice.
- Guess from the local shell or hostname. Neither identifies the remote system reliably.

## Consequences

A host with no reported system uses the existing terminal icon. A connection that disallows the extra exec channel still opens normally.

## Validation

The tests cover distribution and family mapping, persisted hints, and precedence for manual choices. The command can also be run through a local authenticated SSH fixture with a temporary configuration.

## Supersedes

None.

## Revisit when

The host icon needs to reflect the system of each individual connection rather than the last system observed for a host.
