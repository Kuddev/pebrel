# Single ownership of workspace background opacity

## Status
Implemented for local review.

## Context
The background opacity slider appeared almost opaque above 50%, and Settings
ignored it entirely. The surface opacity calculation itself was linear.

## Evidence
The pinned component `Root::render` paints `theme.tokens.background` before its
workspace child. The workspace also paints its own card and shell surfaces.
Two backgrounds of alpha `a` produce effective alpha `1 - (1 - a)^2`: a 50%
preference becomes 75%, and 80% becomes 96%. Settings then painted an opaque
panel over the card. Terminal elements already leave default cells transparent.

## Decision
The product window root explicitly overrides its background to transparent.
Workspace owns the card background, using the terminal palette for terminals
and the theme panel color for Settings. Both apply `card_surface_opacity` once.
SettingsPane paints its controls and text without a second full-page background.
Existing popup panel colors remain opaque. The persisted opacity range and
linear mapping remain unchanged; the setting is labelled window background
opacity in all supported catalogs.

## Rejected alternatives
- A nonlinear slider curve would hide the double compositing and distort saved
  preferences.
- Making all theme background tokens transparent would change unrelated controls.
- Lowering the opacity of the whole view would also fade text and icons.

## Consequences
The slider is more visibly transparent at the same saved value. Settings uses
the same surface alpha and may reveal wallpaper or the native backdrop, while
dialogs and controls retain their existing readability.

## Validation
The focused theme regression covers live 100%, 80%, 50%, 20%, 0% and restoration
for dark and light themes, preserving opaque popup panels and text colors.
Native previews are staged with private configurations for compositor review;
automated alpha assertions do not prove every operating system backdrop.
The Windows GPUI build and 29 focused tests passed, including wallpaper,
update-dialog readability, Settings navigation and internationalization. The
preview binary matched the build and installed settings hashes were unchanged.

## Supersedes
None.

## Revisit when
The component root changes its background ownership, or Settings is presented
outside the workspace card.
