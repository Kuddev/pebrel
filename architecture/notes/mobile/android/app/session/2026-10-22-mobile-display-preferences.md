# Persist phone display choices

## Status
Accepted for implementation.

## Context
Issue #669 reports zoom and phone-width wrapping resetting after leaving a pane.

## Evidence
`TerminalSnapshotView` owned a transient zoom multiplier; `DesktopTerminalScreen`
owned wrapping with `rememberSaveable(identity)`. Neither belonged to the stored
display preferences. Native pinch feedback did not call the existing font setter.

## Decision
Persist `desktop_zoom` and `desktop_wrap_lines` alongside the existing terminal
preferences. Keep zoom as the existing bounded 0.5–5 multiplier so this fix does
not change base font size or resize the desktop terminal. Persist on gesture end
or explicit zoom reset; ignore stale preference reapplication during a pinch.

## Rejected alternatives
Changing the desktop PTY size alters the remote application. Saving only base
font size loses the existing multiplier and can apply zoom twice on reentry.

## Consequences
These reading preferences apply to subsequent desktop panes on this device.
Existing installations default to scale 1 and wrapping off.

## Validation
`DisplayPreferencesTest` performs a real native-view pinch gesture, reapplies
stored preferences during feedback, recreates preferences and the view, and
checks zoom/wrapping restoration plus invalid-scale normalization.

## Supersedes
None.

## Revisit when
Per-host display overrides become an explicit product requirement.
