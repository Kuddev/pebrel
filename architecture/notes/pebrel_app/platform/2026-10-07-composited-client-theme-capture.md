# Composited client capture for theme transitions

## Status

Native capture defect reproduced; focused repair implemented. Current product
compilation, in-process transition and resource-lifetime acceptance remain pending.

## Context

The day/night reveal retained an old client image before drawing the new theme.
The initial adapter copied the window device context with BitBlt. Successful GDI
calls and geometry tests did not prove that this DC contained the GPU-composited
client pixels. An empty image makes the transition fall back to an immediate change.

## Evidence

On an isolated foreground product window, the unchanged candidate adapter returned
an all-black 1140 by 810 image while the window's displayed client area contained
visible content. The same window rendered non-empty client pixels through
PrintWindow with PW_CLIENTONLY and PW_RENDERFULLCONTENT. This was an external
capture probe, not proof of the application's own transition or frame latency.

Microsoft documents [PrintWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-printwindow)
as synchronous: the owning application handles the print request. The pinned
windows-sys 0.59 bindings place PrintWindow and PW_CLIENTONLY under Storage::Xps,
and PW_RENDERFULLCONTENT under UI::WindowsAndMessaging.

## Decision

Request complete client rendering from the existing live window handle instead
of copying its empty GPU-backed DC. Keep one request, the existing DIB ownership,
32 MiB and 4096-pixel admission limits, checked dimensions and opaque BGRA output.
Failed or empty capture retains the existing immediate-theme behavior.

Only the application's own current window is targeted. No desktop capture,
cross-process target selection, capture service or permanently retained frame is
introduced. The GDI DC remains an allocation context, not the source of pixels.
Enable the existing Windows bindings' Xps namespace without adding a package.

## Rejected alternatives

- Repeat BitBlt or lengthen the animation: neither supplies missing client pixels.
- Copy a desktop rectangle: includes occluding or unrelated windows and is not an
  image owned by this workspace.
- Accept a black image: visibly masks the product and falsely reports an effect.
- Add a permanent capture stream or change the renderer for this focused repair
  before testing the existing native full-content operation.

## Consequences

Capture remains a synchronous cold-path operation. The external probe does not
establish latency bounds, responsiveness or in-process reentrancy. Actual product
switching, reversals, resize/DPI changes and cleanup must pass before acceptance.
No per-frame copying or additional capture retry is added. Platform support remains
Windows; other platforms retain immediate switching.

## Validation

External foreground capture reproduced black DC pixels and non-empty full-content
pixels on the same isolated window. The probe closed only its owned process and
verified the resident application's window/pane identities were preserved.
Current-head native CI and the modified product's visual checks remain pending.

## Supersedes

The DC-copy capture choice in [client-frame theme reveal](../gpui_shell/theme/2026-10-06-client-frame-theme-reveal.md).
Workspace image ownership, reveal geometry and teardown requirements are unchanged.

## Revisit when

In-process validation exposes reentrancy or blocking, native full-content capture
returns incomplete pixels, or GPUI gains a supported production GPU snapshot API.
