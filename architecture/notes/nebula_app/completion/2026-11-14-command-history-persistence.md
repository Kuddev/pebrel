# Keeping command history in memory only

## Status

Proposed in the change for issue #988.

## Context

Pebrel appends every recorded command to `pebrel_history.jsonl`,
`pebrel_history_wsl.jsonl` or `pebrel_history_ssh.jsonl`. Commands typed in a
remote SSH session therefore land on the local disk. The Settings page could only
clear those files after the fact. Some users want suggestions during a session
without leaving a plaintext record behind.

## Evidence

[`nebula_history.rs`](../../../../nebula_app/src/nebula_history.rs) writes history
in one place, `append`, called from `NebulaHistory::record`. Loading, ranking and
clearing never write a history file. The clear operation from
[2026-10-23](2026-10-23-command-history-clear.md) already owns deletion.

## Decision

Add `command_history_persist` to the shared settings model, defaulting to true so
existing configurations keep writing history. `nebula_history` holds a
process-wide flag that `append` checks before opening a file. The GPUI
application sets the flag from the settings global during startup and updates it
from an observer on that global, so open terminals follow the switch at once.

With the switch off, new commands still enter the in-memory pools and appear as
suggestions until Pebrel exits. Existing files still load at startup and are not
rewritten or deleted; the existing clear button remains the way to remove them.
One switch covers local, WSL and SSH history.

## Rejected alternatives

- A second switch for SSH history only: adds a settings row and messages for a
  case the global switch already covers. A per-scope rule can extend the same
  gate later if users ask for it.
- Ignoring saved files while the switch is off: hides history the user chose to
  keep and makes the switch look like a delete. Clearing stays explicit.
- Deleting files when the switch turns off: an irreversible action behind a
  toggle, without the confirmation dialog the clear button has.
- A field on `NebulaHistory`: the shared history loads lazily on first use, and a
  derived default of false would silently stop persistence. The flag starts on.

## Consequences

The settings file gains one boolean key. Missing or invalid values keep writing
history, and reset restores it. The legacy renderer never applies the setting and
keeps writing history. Opt-in debug logging (`PEBREL_DEBUG_LOG`) still records
commands in `pebrel_debug.log`; it is a diagnostic channel outside this setting.

## Validation

Settings tests cover defaults, invalid values, round trips and reset. A history
test turns the flag off and checks that a recorded command stays in memory and
does not reach the history file. A GPUI test clicks the visible switch twice and
checks the saved value and the settings global.

## Supersedes

None.

## Revisit when

History storage moves to an encrypted or per-session format, or users need
separate persistence rules per scope.
