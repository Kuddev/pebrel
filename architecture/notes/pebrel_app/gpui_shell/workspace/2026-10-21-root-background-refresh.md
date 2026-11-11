# Preserve transparent root across background refreshes

## Status
Implemented for local review.

## Context
The first opacity fix cleared Root at window creation. Native Windows previews
still became visually opaque at 50% and above, despite correct card alpha values.

## Evidence
`wallpaper::refresh_surface_opacity` schedules `sync_root_background`. That
function previously kept an explicit transparent Root only while a native video
underlay was ready. Otherwise it set `style.background = None`, restoring the
component's full-window theme background and undoing the creation-time override.
The pinned Windows renderer accumulates background alpha additively, so two
surfaces at 0.5 reach alpha 1. This explains the hard threshold left after the
earlier single-surface correction, beyond normal source-over compositing.

## Decision
Keep Root's explicit transparent fill on every background refresh, independently
of wallpaper readiness or kind. Workspace remains the sole owner of shell/card
surface fills. Preserve the global component background token for controls and
dialogs, the existing saved opacity values, and the linear slider mapping.

## Rejected alternatives
- A nonlinear curve or halving user opacity would conceal the duplicate fill.
- Clearing the global background token fixes the native symptom but changes
  unrelated controls and default component dialogs.
- A window-creation-only override is overwritten by the existing refresh path.

## Consequences
Opacity continues to work after live changes, theme reloads, wallpaper loading,
clearing and switching. Text and independently painted dialogs remain unchanged.

## Validation
The regression drives the actual deferred root synchronization without a native
wallpaper at 100%, 80%, 60%, 40%, 0% and restoration to 100%.
Private native previews over a fixed white/black background compare the same
Windows compositor path before and after the correction. Probe outputs and
screenshots remain under ignored `tmp/`.
Before the fix, 80% and 60% measured as fully opaque, while 40% measured as 80%.
After the fix, the same probe measured 80%, 60% and 40% respectively. The Windows
build and nine focused wallpaper, surface and dialog regressions passed.

## Supersedes
Completes the lifecycle contract in [single background opacity](2026-10-21-single-background-opacity.md).

## Revisit when
Component Root changes its default fill or Windows changes its blend policy.
