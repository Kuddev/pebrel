# ConPTY startup erases the scrollback of a waking tab

## Status

Accepted for the stable terminal.

## Context

Waking an idle tab starts a new PTY in its saved directory. The previous shell's output belongs in the primary grid's history. The new shell owns its cursor and screen.

## Evidence

On Windows x64 with Pebrel 2.5.9, a PowerShell tab slept after 308 seconds. Its replacement started with nine history rows. The PTY sent ED2 at byte 32 and ED3 at byte 36, before the first OSC 133 prompt mark at byte 339. History dropped to zero. A directory listing and a separate stdout marker both disappeared. [Windows trace](https://github.com/Kuddev/pebrel/actions/runs/44496914899)

## Decision

Hold the bounded snapshot during Windows PTY startup. Install it once, after the first primary-screen ED3 has run or immediately before recording the first primary prompt. If the shell has already printed a banner, retain its rows and fill the remaining history capacity with the newest saved rows.

## Rejected alternatives

Ignoring ED3 would change explicit history clearing. A timer would depend on shell and machine startup time.

## Consequences

The host can initialize a clean screen and later clears operate normally. Unix PTYs still receive the snapshot before reading output. The in-memory snapshot has one owner throughout the handover.

## Validation

The VT cases cover every split of the captured ED2/ED3 pair, an alternate-screen clear, a later explicit clear and a startup banner with limited history capacity. The original Windows issue also checks a clean workspace restart. [Issue #992](https://github.com/Kuddev/pebrel/issues/992)

## Supersedes

The Windows restore timing in the [local tab hibernation decision](../../nebula_app/gpui_shell/workspace/2026-11-14-local-tab-hibernation.md).

## Revisit when

The Windows PTY exposes an explicit startup-complete boundary before shell output.
