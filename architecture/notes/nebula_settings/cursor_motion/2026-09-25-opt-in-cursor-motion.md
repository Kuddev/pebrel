# Opt-in cursor motion preference

## Status
Accepted for local implementation, 2026-09-25.

## Context
Expose smooth terminal cursor movement as a dropdown under Appearance / Cursor without changing existing installations.

## Evidence
RuntimeSettings owns persisted runtime preferences. SettingsPane persists keys and broadcasts Changed, and the workspace hot-applies Settings to open TerminalViews. The existing notification-duration selector demonstrates failure rollback.

## Decision
Add the shared enum CursorMotion with stable values `off` and `smooth`. Missing, empty or invalid values resolve to Off. Smooth means the fixed 90 ms silkmux trajectory. Include the key in preference reset. The GPUI adapter reads cached settings and applies changes to existing panes; no PTY restart is involved.

## Rejected alternatives
A boolean loses the explicit option contract. Multiple duration settings are unnecessary for reproducing the approved effect. A new Lua/TOML override would introduce a second source and priority policy without a user need.

## Consequences
Old settings retain instant movement. Saving does not modify cursor shape/blinking or unknown keys. Failed writes restore the visible selection and show an error. Other shells can ignore the additive preference.

## Validation
Shared default/invalid/round-trip/reset tests plus GPUI selection, error rollback, localization and hot-application tests.

## Supersedes
None.

## Revisit when
Additional user-visible motion modes or a deliberate default change are requested.
