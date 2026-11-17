# Pointer chord turns must not overlap a render burst

## Status

Implemented in the PTY harness (`scripts/tests/test_powershell_pointer.py`).
The chord transport itself is unchanged from
[the native PSReadLine pointer decision](2026-10-31-powershell-native-pointer.md);
this note records a harness contract and the load-fragility evidence behind it.

## Context

CI (macos-26-intel) failed `test_native_unicode_moves_range_delete_and_one_undo`
sporadically while the same test passed on the same runner image at other times
of the same day. The pointer protocol drives PSReadLine with ESC-prefixed CSI
chords (`Ctrl+Shift+F10,digit`, `Ctrl+Shift+F12`) written over a real PTY, and
the harness schedules its next chord after a wall-clock quiet window.

## Evidence

A byte-level trace (both PTY directions, timestamps) of a failing run showed:

- The numeric query completed normally (`pebrel_pointer` var arrived).
- The move chord and the snapshot chord were written 304 ms apart with **zero
  output between them**: pwsh had not yet reacted to the move chord when the
  snapshot chord landed inside its in-flight render burst.
- The subsequent renders show the snapshot chord torn: a standalone Escape
  (Windows mode reverts the line — the buffer render collapses to spaces),
  then literal `[` and `;` keys inserted (digits swallowed by DigitArgument),
  and the `pebrel_editor` var never emitted; the harness timed out after 12 s.

Reproduced under a 4-way parallel stress loop on macOS x64: 36/40 runs failed,
23 of them with exactly this tear signature (matching the CI assertion and the
captured buffer residues). The same test passes deterministically without
parallel load. In every passing turn the chord was parsed only after the
previous burst had drained (probe, render, quiet).

## Decision

The harness serializes protocol turns: before writing any chord it waits
(bounded) for the editor's first reaction byte to the previous write, then
drains the burst to quiet (`PowerShellSession.turn`). The wait-loop poll
interval drops from 100 ms to 20 ms so cursor-position replies land closer to
real-terminal timing. Test assertions are unchanged; no coverage is removed.

## Rejected alternatives

- Retry or restart the session on corruption: masks the tear instead of
  removing its cause, and could hide a genuinely corrupted buffer behind a
  fresh one.
- Longer wall-clock quiet windows: the quiet window expired mid-burst in the
  failing trace (304 ms of total silence), so tuning the window alone cannot
  anchor on the editor's actual progress.
- Changing the protocol chords: unbound CSI chords necessarily carry an ESC
  prefix, and the same encoding is what the GUI negotiates through its
  keyboard encoder; there is no non-ESC alternative in the xterm space.
- A background reader thread for cursor-position replies: the failing trace
  shows replies arriving 1 ms after each probe, so reply latency was not the
  cause; threading would rewrite the harness without addressing the overlap.

## Consequences

Each chord is parsed from a quiescent input queue, and the harness now states
its turn discipline in code rather than relying on runner idle time. Turns
cost one reaction wait plus the drain window, so the suite runs somewhat
slower under load. The production exposure is unchanged and documented: the
GUI sends the same chords over a real PTY, so the same tear remains possible
under extreme load; the protocol's query/snapshot replies let the GUI detect a
desynchronized editor, and no field occurrence is known.

## Validation

`scripts.tests.test_powershell_pointer` passes. Under the identical 4-way
parallel stress: before the change 23/40 runs showed the tear signature,
after the change 0/40 (the remaining 8 are pwsh cold starts starved past the
12 s startup timeout by four concurrent instances — an artifact of the
artificial parallel load, never observed in CI's serial native jobs).

## Supersedes

None. Evidence amendment for
[the native PSReadLine pointer decision](2026-10-31-powershell-native-pointer.md).

## Revisit when

A tear is observed outside the test harness, PSReadLine or .NET documents a
reliable ESC-sequence read contract, or the GUI starts batching pointer chords
with its own turn scheduling.
