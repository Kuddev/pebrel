# Ordinary-window startup geometry

## Status

Proposed for review.

## Context

The GPUI startup path already reads explicit Lua grid dimensions, but ignores
`window.position`. Regular workspace snapshots omit `Session.window`, so the
last size and position disappear when the process exits. Quick Terminal already
owns an independent remembered size and monitor policy.

## Evidence

`workspace/windowing.rs` owns the registry and combined persistence boundary.
`session/window_layout.rs` preserves per-window metadata in the same atomic JSON
file as tabs. The pinned GPUI Windows backend (`fc05d637`) obtains normal bounds
with `GetWindowPlacement`; maximized and fullscreen bounds retain their restore
rectangle. Windows placement uses workspace coordinates, whereas display bounds
and configured positions use desktop coordinates.

## Decision

Attach normal native geometry at the ordinary-window persistence boundary,
including empty tab-close snapshots. Cold restore belongs only to the first
ordinary `RestoreOrDefault` window. New windows, Quick Terminal and isolated
administrator processes never consume this geometry. Geometry restoration is
independent of restoring shell tabs; the boot-attempt cutoff still prevents a
broken saved startup loop. Existing empty-session and background-close policies
remain owned by session persistence.

Sizes remain logical GPUI pixels. Windows saved position is the physical desktop
client origin, paired with the stable display UUID. Restore first chooses the
configured physical monitor, otherwise the remembered display, otherwise the
nearest physical monitor, then the primary display. It clamps size and origin
within that display's work area. Working coordinates are translated only at the
Windows placement boundary. Fullscreen saves its normal rectangle but does not
force a fullscreen startup; maximized state may be restored.

Explicit Lua dimensions override saved size; explicit Lua position overrides
saved position. Missing dimensions are not an implicit explicit default.
Configured Windows position describes the outer window in physical desktop
pixels, as the legacy configuration does. Apply it using `SetWindowPos` because native border insets are known only after
creation. Hidden Windows startup retains a one-shot position until its first
active frame, after GPUI consumes its cached native placement; applying it
in the builder would be overwritten by activation.
Other platforms keep their existing position behavior pending an equivalent
public/native placement contract.

## Rejected alternatives

- Persisting zoom-derived rows/columns: couples layout to font state and loses
  the user's actual native size.
- Saving maximized/fullscreen viewport size: destroys the normal restore size.
- Always recentering on primary: discards a still-attached secondary display.
- Reopening all ordinary windows for cold startup: changes tab restoration policy.
- A second geometry file: adds a competing save lifecycle and atomicity boundary.

## Consequences

New JSON fields are optional; older sessions remain readable. The shared session
format still contains flattened tabs and per-window layouts, without a version
bump. Quick Terminal settings remain separate. Native DPI conversion follows the
pinned backend and must be reviewed when that dependency changes.

## Validation

The added GPUI snapshot regression exercises a real registered workspace and
normal native bounds. Session tests atomically roundtrip multiwindow and empty
geometry, accept old metadata, and enforce the crash-loop cutoff. Startup tests
cover explicit grid priority and selected-display offscreen clamping. Execution
and platform acceptance are performed in GitHub Actions; local execution is not
part of this change's evidence.

## Supersedes

None.

## Revisit when

GPUI exposes a cross-platform display scale/position API or cold startup begins
restoring separate ordinary windows.
