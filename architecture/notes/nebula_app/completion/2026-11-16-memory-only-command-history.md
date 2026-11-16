# Keep command history in memory by preference

## Status

Implemented.

## Context

Completion shares one command-history model across local, WSL, and SSH shells.
Before the preference, new entries always appended to disk. Existing history
also provides suggestions and has an explicit deletion action.

## Evidence

The command-history integration has one append boundary after the scope and
command have been validated. The three categories share that boundary, so a
single preference can cover them without changing the file format.

## Decision

Read `command_history_persist` when loading history. `command_history_persist=0`
in `pebrel_settings.txt` keeps new entries in memory after relaunch; existing
files remain readable. An unreadable settings file also disables new writes.
The choice belongs to the history instance and the settings reader.

## Rejected alternatives

A process-global write switch makes independent stores and tests depend on one
another. Erasing or ignoring existing files on opt-out would change the meaning
of the separate history deletion action.

## Consequences

Saving remains enabled by default. Suggestions continue to work during the
current process, and old files and the clear-history behavior are preserved.

## Validation

The settings test covers defaults, save, and reset. History tests cover scoped
suggestions when disk persistence is disabled.

## Supersedes

None.

## Revisit when

The application supports changing command-history persistence during a session.
