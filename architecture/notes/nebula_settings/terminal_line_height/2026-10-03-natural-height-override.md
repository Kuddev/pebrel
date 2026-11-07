# Natural terminal line-height override

## Status
Proposed in the Issue #439 implementation.

## Context
Users need independent terminal spacing without editing a custom theme.

## Evidence
Terminal typography previously used shaped ascent plus descent, or the custom
theme's font-size multiplier, followed by the configured device-pixel offset.

## Decision
`terminal_line_height` is optional, finite, clamped to 0.50–5.00, and rounded to
two decimal places by nebula-settings. Explicit values multiply natural shaped
font height and override theme line height. Auto removes the setting and preserves
existing theme/natural geometry. The editor starts at 1.00; no default migration
changes existing spacing. Device-pixel offsets and rounding remain unchanged.

## Rejected alternatives
A default 1.40 would silently expand existing grids. Reusing the theme key would
change its established font-size semantics. Separate pointer/selection geometry
would duplicate the terminal cell-height authority.

## Consequences
Startup and live layout call the same typography function. Prepaint writes the
new cell height into the grid, PTY viewport and pointer/selection layout together.
Existing settings reset removes the explicit override.

## Validation
Regression coverage checks persistence, precision/range, reset, editor commit/
cancel/blur, theme precedence and device-pixel geometry. Execution is delegated
to GitHub Actions; native visual acceptance remains separate.

## Supersedes
None.

## Revisit when
Font metrics or theme line-height semantics change.
