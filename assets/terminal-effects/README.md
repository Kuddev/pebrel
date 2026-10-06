# Terminal WGSL effects

These sources use the application's terminal-effect ABI. Add `.wgsl` files
in **Terminal effects**, arrange their order, then enable the chain separately from
the background. Adding files disables execution until enabled again. Moving or
removing existing files preserves activation; removing the last file disables it.
Reload explicitly recompiles all files. Windows product builds include this
support by default; sources still require explicit activation. Other native
backends are not yet supported.

The chain supports up to eight files and eight total `@fragment` entry points.
Files run in list order; entries within each file run in declaration order;
each pass reads the previous pass's result. Input and output are
local physical pixels with a top-left origin. The renderer uses two reusable
full-resolution textures and does not reduce terminal text resolution.

The application prepends these inputs; do not redeclare them:

- `surface`: the visible terminal texture. Pixels excluded by the content mask
  are transparent. `load_surface(vec2<i32>)` reads a clamped pixel;
  `sample_surface(vec2<f32>)` samples normalized coordinates with clamp-to-edge
  bilinear interpolation. These helpers avoid backend-specific sampler heaps.
- `frame.viewport`: width, height, inverse width, inverse height.
- `frame.time`: elapsed seconds, delta seconds, last cursor-state change time,
  last focus change time. Time is monotonic; disabling animation stops automatic
  redraws, not time observations on later content changes.
- `frame.flags`: ABI version (1), frame-snapshot ordinal, focus flag, cursor visibility.
- `frame.cursor` / `frame.previous_cursor`: x, y, width and height.
- `frame.cursor_color` / `frame.previous_cursor_color`: RGBA values.
- `frame.cursor_styles`: current style, previous style, previous visibility, reserved.
  Styles: block 0, hollow 1, bar 2, underline 3, hidden 4.
- `frame.foreground`, `background`, `cursor_text`, `selection_foreground`,
  `selection_background`: resolved/configured RGBA color roles.
- `frame.palette[256]`: resolved ANSI/indexed RGBA palette, including OSC overrides.

The texture is premultiplied RGBA. Preserve alpha and premultiplication when
modifying colors. Effects run before completion popups and IME preedit overlays.
Additional shader resource bindings are not part of this ABI.

`cursor-trail.wgsl` demonstrates cursor state. `scanline-vignette.wgsl` demonstrates
ordered passes. Neither is enabled automatically, and neither changes saved theme
or background preferences.
