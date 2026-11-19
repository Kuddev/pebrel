# Preserve the phone's desktop display preferences

## Status

Implemented for review with Robolectric regression coverage. Physical Android
device acceptance remains separate.

## Context

Issue #669 reports that leaving and reopening a mirrored desktop pane resets both
pinch zoom and the phone-width wrapping option. The screen owned wrapping through
`rememberSaveable`, while `TerminalSnapshotView` owned zoom only in the View.

## Evidence

`DesktopTerminalScreen` already observes `DisplayPreferences`, which persists the
global base font size and pinch enablement. The native mirror multiplies that base
size by an independent 0.5–5 scale. Its size indicator reports an integer effective
font size; the legacy text renderer instead persists a base font size from 8–32.

## Decision

Persist `desktopWrapLines` and `desktopZoom` in the existing display preferences.
They are global display choices, like the existing base font size; no pane content,
connection identity or per-pane settings registry is persisted. Existing users
retain wrapping disabled and scale 1 until they explicitly change them.

Keep the native mirror's precise scale separate from the base font size. Commit it
at gesture completion and at the existing blank-space double-tap reset. Restore
without emitting another user change, and ignore stale preference application
during an active pinch. Share scale normalization between persistence and the View.
The existing legacy text size persistence remains intact.

## Rejected alternatives

- Saving the rounded indicator size into the base font preference: clips the native
  scale range and multiplies an already scaled size again on recomposition.
- Keeping state only in `rememberSaveable`: does not establish durable preferences
  after navigation or creation of a new repository/View.
- Writing every scale sample: creates unnecessary preference writes while the View
  already owns the active gesture.

## Consequences

New mirror Views restore both choices, including after a new `SessionRepository`
loads disk preferences. Reflow remains a phone-side projection and does not resize
the desktop PTY. Existing pixel feedback and single-finger interaction are retained.

## Validation

`DesktopOutputSurfaceTest` exercises the actual screen menu, native pinch, removal,
repository recreation and second pinch. `TerminalSnapshotViewTest` covers exact
scale restoration, stale reapplication during a gesture, rendered glyph size,
double-tap reset, range normalization and preservation of the base font size.
Run these suites together with `CommandComposerTest` on a host supported by
Robolectric's native graphics runtime. The reconciliation with current main retains
its `zoomScale` callbacks and active-pinch protection; CI results for that revision
are recorded in the pull request.

## Supersedes

None.

## Revisit when

Users explicitly require independent display preferences per device or pane.
