# File-only tab bar material overrides

## Status

Implemented; native compositor appearance still requires physical OS validation.

## Context

Sidebar tabs and top tabs previously shared the window's opacity and native
backdrop with terminal cards. The maintainer requested one optional tab-region
configuration, with no new settings-page controls and no default appearance change.

## Evidence

- `wallpaper.rs` prepares window opacity and blur; `theme.rs` applies that opacity
  to shell tokens and terminal cards.
- `workspace/sidebar.rs` paints the left tab surface. `window_titlebar.rs` paints
  the top-tab surface and the titlebar band above a sidebar.
- `platform/window_material.rs` and GPUI expose one native backdrop per window.
  The existing APIs cannot apply two different native blur materials to two
  simultaneously translucent regions.

## Decision

`nebula_settings::TabBarMaterial` owns the optional `tab_bar_opacity` and
`tab_bar_blur` keys. Missing, `inherit`, and invalid values inherit the window
preference; opacity accepts finite values clamped to 0..1. Explicit `none` is a
valid blur override. Both tab positions use the same prepared state and background
helper. Text and icon alpha are not modified. Reset removes both overrides.

Edit `pebrel_settings.txt` and restart the application to load file-only changes.
For a translucent, blurred tab bar with opaque content:

```text
opacity=1
blur=none
tab_bar_opacity=0.65
tab_bar_blur=acrylic
```

Window-level opacity/blur remain authoritative for non-tab surfaces. When those
surfaces are opaque, the requested tab backdrop can be composed under the window
and covered everywhere else. If non-tab surfaces become translucent and request
a different material, retain their window backdrop and log the unsupported
combination once per conflict transition. Independent tab opacity still applies.
Matching blur modes work with either opacity. Wallpaper readiness and live opacity
changes reevaluate this choice without decoding files or rebuilding native
controllers on every frame. Existing per-window material application gating stays
in place. macOS/Linux continue using their existing native blur/compositor support;
Windows-specific material names do not promise identical visuals on other systems.

## Rejected alternatives

- Change the global opacity: it also makes terminal content translucent.
- Set opacity on the entire tab element: it fades text and icons.
- Unconditionally replace the native backdrop with `tab_bar_blur`: it silently
  changes translucent terminal content, defeating the requested region boundary.
- Add an unverified native child window or a second compositor: this would add
  input, clipping, resize and lifecycle behavior outside this configuration change.

## Consequences

Defaults retain the shared whole-window behavior. One pair of file-only keys
controls both tab layouts. Independently selecting two different native materials
while both regions are translucent is explicitly unsupported by the current
backend. Settings-page controls and theme import/export format are unchanged.

## Validation

Settings tests cover defaults, invalid/nonfinite input, bounds, explicit `none`,
both tab positions, round trips, unknown-key preservation and reset. Application
tests cover tab alpha independence, live opacity transitions and the native
material conflict policy. Production compilation and architecture checks validate
the shared code path; these checks do not prove physical native-blur appearance.

## Supersedes

None.

## Revisit when

The platform abstraction exposes verified per-region native backdrops, or settings
UI / theme-file support is requested separately.
