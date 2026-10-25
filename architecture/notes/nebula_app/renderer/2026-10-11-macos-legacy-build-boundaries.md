# Explicit macOS legacy compilation retains the shared display contracts

## Status

Proposed narrow build repair following explicit macOS `legacy-shell` review.

## Context

The normal GPUI build selects `product_ui`, `product_renderer` and
`product_window_context`. An explicit legacy build selects the original display,
renderer and window orchestration modules. Passing normal-product compilation
therefore does not establish that the legacy adapters still compile.

## Evidence

At upstream `ef70dcae83657427aa968c22070e451205e8116f`, the macOS window builder
uses `Decorations` without importing its type. The GPUI wallpaper consumer imports
`wallpaper_rect` through `renderer::image`; that path is public in the GPUI shim
but privately imported in the legacy image module. The legacy runtime palette
closure has an ambiguous index even though the shared terminal-read functions
require `usize`. These sources and feature-selection declarations were unchanged
in the SSH-picker branch when its explicit legacy check reported six errors.

## Decision

Import `Decorations` inside the existing macOS import group, re-export the existing
wallpaper layout function with crate visibility, and annotate the palette index
as `usize`, matching the existing terminal-read API. Keep one image-layout
authority and the existing color lookup, rendering and shell selection.

## Rejected alternatives

- Copy the layout helper into GPUI: that would introduce a second layout authority.
- Suppress legacy modules or compile errors: that would hide the unverified path.
- Expand into renderer migration: the failures require only type and visibility
  corrections, with no ownership or runtime redesign.

## Consequences

The feature-selected legacy adapters can consume the existing shared contracts.
No dependency, runtime behavior, configuration, persistence, credential or thread
model changes. The wallpaper helper remains crate-scoped.

## Validation

On macOS Apple Silicon with fixed Rust 1.97.1 and locked dependencies, the clean
ef70 baseline reproduced all six errors with `gpui-shell,legacy-shell` and the
existing `gpui_platform/runtime_shaders` feature. The repaired identical feature
check and normal GPUI product build both passed. Fixed-toolchain format checking,
the architecture checker against ef70, platform-cfg 468/468, diff checking and
both source-budget tests passed. No native Windows or Linux runtime coverage
follows from macOS compilation; mirror tests of imports or annotations add no
behavioral evidence. This CLT-only host cannot perform the ordinary Metal build;
the existing runtime shader feature supplies the local product build route.

## Supersedes

None.

## Revisit when

The legacy entry is removed or another consumer changes the shared layout/color
contracts or supported feature combinations.
