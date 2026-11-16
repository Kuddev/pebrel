# Write Pebrel names, keep reading the Nebula ones

## Status

Accepted with the maintainer-requested Pebrel naming migration, 2026-11-09.
Extends [the 2026-09-07 product rename](../../../../docs/architecture-decisions.md),
which renamed executables, configuration files and the data directory.

## Context

The maintainer asked for every remaining `nebula` name to become `pebrel`, while
keeping compatibility. Some remaining names are stored on user machines, set by
users, or exchanged between processes that may come from different installs.

## Evidence

- `ai_sessions.json` and `agent-detection/` were still resolved by their own
  helpers under `%APPDATA%\Nebula` (or `~/.config/Nebula`), outside the startup
  migration list, so they never moved to the Pebrel directory.
- Fonts imported before the rename stay in `Nebula\fonts` for the same reason.
- Windows Credential Manager holds the sync secrets under `Nebula Sync …` names.
- Settings files store `theme=Nebula`; exported backups use `.nebula-backup`.
- Remote hook bridges are installed once per host and shared by every client
  version that connects; shell scripts in the data directory are rewritten by
  whichever installed version starts last.

## Decision

New writes use Pebrel names; old names stay readable:

- Session registry and agent-detection overrides: copied once from the old
  location when the new one is missing, never overwriting or deleting.
- Imported fonts: also read from `Nebula\fonts`.
- Sync secrets: read the Pebrel target, then the Nebula one; saves use Pebrel.
- Theme key: written as `Pebrel`, `Nebula` still parses. Backups export as
  `.pebrel-backup` and workspaces as `.pebrel-workspace.json`; open dialogs
  accept both.
- Environment variables read by the app use `PEBREL_*`; startup already maps
  `NEBULA_*` to `PEBREL_*` before any reader runs. `PEBREL_WINDOW_ID` is set
  alongside `NEBULA_WINDOW_ID` for old `msg` clients in the same shell.
- terminfo: `pebrel` first, then `nebula`, then `xterm-256color`.
- Parsers accept `PEBREL|` and `pebrel-hook` alongside the current markers.

Emitters of `NEBULA|`, `nebula-hook/1` and `777;nebula-hook;` stay unchanged:
an older install receiving a new marker would drop every cwd, branch and agent
event. Lock-file suffixes (`nebula-lock`) also stay: `atomic_file` locks
serialize writes to shared files such as the settings file, and two installs
running at once only exclude each other if they use the same lock name.
Switching emitters needs a release in which every supported reader already
accepts both names.

## Rejected alternatives

- Text replacement of every name: breaks stored data and mixed-version pairs.
- Moving old data instead of copying: a downgrade would lose it.
- Changing emitters now: readers in installed releases accept only old markers.

## Consequences

The old names remain as documented inputs. Inno AppId, the credentials prefix
and data-file migration already had compatibility and are unchanged.

## Validation

Unit tests cover copying without overwriting, both theme keys, both hook headers
and OSC tags, and both title markers. Windows credential fallback is covered by
native CI only to the extent it compiles; no Windows credential test was run.

## Supersedes

None.

## Revisit when

Every supported release accepts `PEBREL|` and `pebrel-hook`; then emitters can
switch.
