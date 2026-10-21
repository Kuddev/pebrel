# Static tab overflow and overlapping Chrome feet

## Status
Implemented.

## Context
Long names must stay still when selected or hovered and fade out at the trailing
edge. Chrome hover and selected surfaces should be close despite the 12px flared
corners. Tabs use spacing alone rather than separator lines.

## Evidence
[The label painter](../../../../../nebula_app/src/gpui_shell/workspace/top_tabs/label.rs)
uses GPUI content masks and opacity on a containing Div. Canvas style opacity is
not applied by GPUI's Canvas painter; Div applies it to both monochrome glyphs
and color emoji. Overpainting a gradient with a theme background would
hide wallpaper or show a rectangle with translucent/custom themes.

## Decision
Remove title animations and their hover state. A frame-local shared shaped line
paints a solid area and 24 clipped opacity bands over the final 48 logical
pixels. It retains the normal text height and full accessible name. No animation
frames, timers, or persistent caches are requested by labels.

Chrome tabs overlap by 10px, leaving about 2px between a neighboring hover surface
and the selected tab's inset body. The shared strip geometry stores this gap for
layout, navigation and frozen drag snapshots. Selected tabs paint after inactive
neighbors; a dragged tab paints above both. No separator lines are drawn between
tabs. Other tab styles retain their existing gap.

The shell indicator uses the trailing status slot; it is visible on
the selected tab and yields to the hover close button.

A live preview exposed an out-of-bounds read when Runtime API tab creation
changed the tab count during a pending drag. Reconcile the frozen snapshot before
rendering or updating the pointer position, canceling it when the count changes
or the source disappears. The renderer also rejects mismatched snapshot counts.

## Rejected alternatives
Opaque fade overlays would change the underlying surface. Per-glyph colors would
not fade color emoji. Moving only the background without changing geometry would
leave hit testing and drag positions inconsistent with the displayed tabs.

## Consequences
Overflow remains static in every interaction state. Glyph shaping is shared
within each label's paint; the trailing fade uses a fixed number of clip bands.
Chrome content has 20px horizontal padding: 12px for the flared feet plus 8px
inside the visible body. Overlap stays outside its controls.

## Validation
[Rendered interaction tests](../../../../../nebula_app/src/gpui_shell/workspace/settings_navigation/ui_tests.rs)
cover static fading, hover close hitboxes, absence of separators, overlapping
tab bounds, content padding, count changes during pending gestures
and bidirectional dragging. Shared geometry tests cover Chrome's
negative gap alongside the existing positive-gap navigation cases.

## Supersedes
The scrolling behavior in [interactive tab titles](2026-10-20-interactive-tab-titles.md).

## Revisit when
Tab padding/corner sizes change or GPUI supplies a native gradient content mask.
