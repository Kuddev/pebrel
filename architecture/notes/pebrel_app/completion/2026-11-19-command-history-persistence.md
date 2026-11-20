# Configurable command history persistence

## Status

Proposed for #988. The setting and reset contract come from #991.

## Context

Local, WSL and SSH commands feed scoped suggestions. SSH command text can otherwise accumulate on the local disk.

## Evidence

Both application shells load their command pools through `NebulaHistory::load`. Each accepted command reaches `NebulaHistory::record` before the JSONL append.

## Decision

`command_history_persist=0` in the settings file keeps newly recorded commands in memory. History loads the configured write directory once. A missing or invalid setting preserves the existing default of appending commands. File edits take effect on restart.

## Rejected alternatives

A process-wide mutable switch would require synchronizing the writers and the tests. Binding the destination to the history instance also covers the legacy shell.

## Consequences

Existing history remains searchable. An explicit history clear still removes saved commands. Re-enabling persistence writes subsequent commands. Shell-owned history, directory history and opt-in diagnostic logs have separate lifecycles.

## Validation

The storage regression uses a temporary directory for all three scopes and checks file contents, suggestions, restart and resumed writes. Settings tests exercise missing, invalid, disabled and reset values.

## Supersedes

None.

## Revisit when

An in-app control needs to switch existing history writers while the process is running.
