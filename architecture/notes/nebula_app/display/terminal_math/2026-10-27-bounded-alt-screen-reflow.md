# Bounded inline reflow on the alternate screen

## Status

Proposed; covered by scanner, projection and GPUI frame regressions.

## Context

Claude Code's full-screen renderer (`"tui": "fullscreen"`) and similar agent
TUIs draw their answers on the alternate screen. Both shells disabled the
compact inline projection there (`reflow_inline: !alt_screen`) so that TUI
geometry such as sidebars, cards and status bands keeps its columns. A
rendered inline formula is much narrower than its TeX source, so every
answer line kept a hole as wide as the hidden source after each formula.

The same answers also showed formulas that stayed literal although the
rest of the screen rendered: `$C_1$`, `$C_2$`,
`$y=C_1e^{r_1x}+C_2e^{r_2x}$` and `$(0,f(0))$`.

## Evidence

Screenshots of a Claude Code full-screen answer in Pebrel 2.3.1 show inline
formulas followed by gaps of 10–20 columns, and a table whose formula cell
would drag its right border if the main-screen projection were applied
unchanged.

The scanner regression for the reported line returned only `f(x_0)>0` and
`f^{\prime}(x_0)=0`: `C_1` consists of uppercase letters, digits and `_`, so
the shell-variable filter that keeps `$LONG_VARIABLE$` literal rejected it.
`C_1e^{r_1x}` is an implicit product of more than three letters without a
TeX command, and `(0,f(0))` a tuple whose item is a function value; inline
evidence accepted neither.

A cursor-row exclusion was also suspected. The runtime screen read of the
live Claude Code pane reported a visible cursor on the input row (`❯`), not
on an answer row, so the exclusion is unchanged.

## Decision

Both screens reflow compact inline formulas. Each single-row inline formula
carries a `reflow_limit`: the first column after it that is a box-drawing or
block element (U+2500–U+259F), an ASCII `|`, the first glyph after two or more
blank cells, or a cell with its own ANSI background. Only cells between the
formula and that column follow the compacted formula; the limit column and
everything after it keep their terminal column. A later formula past the
limit starts its own shift. Prose pushed right by a formula wider than its
source is clipped at the limit instead of covering it, and the formula box is
capped at the limit. Hit testing maps cells vacated in front of the limit
back to the last source cell before it.

A shell variable needs at least two letters, so a single capital with a
subscript (`C_1`, `X_0`) is math while `$AB$`, `$A1$` and `$LONG_VARIABLE$`
stay literal. Inline evidence additionally accepts a braced script
(`^{…}`/`_{…}` after a non-empty base) and tuple items that are
single-letter function values, split on top-level commas. Bare delimiters
keep their stricter TeX-command requirement.

## Rejected alternatives

- Gate alternate-screen reflow on detecting an agent process: WSL and SSH
  sessions only expose `wsl.exe` / `ssh.exe`, which the scan already refuses
  to rely on, and agent TUIs draw the same borders and columns.
- Shift whole alternate-screen rows as on the main screen: moves table
  borders, pane separators and right-hand columns.
- Ask users to leave the full-screen renderer: hides the defect, and the
  table-border problem also exists on the main screen.
- Ask the emitting model to avoid inline math or to add TeX commands:
  shifts a recognizer limitation onto every answer.
- Ignore a hidden cursor on the alternate screen: not supported by the
  live pane, whose cursor is visible on the agent's input row.

## Consequences

Agent answers on the alternate screen close the gap after inline formulas.
Main-screen rows gain the same stop columns, so a formula inside a table
cell no longer drags the right border. Prose separated from a formula by two
or more spaces keeps a gap there. Text that a wider-than-source formula
would push onto a stop column is not drawn on that row. Terminal cells,
copied text and protocol cursor positions are unchanged; the projection
remains a sparse per-formula mapping.

## Validation

`display/terminal_math/reflow_tests.rs` covers table borders, gap-aligned
columns, wide-glyph spacers, borders between two formulas, positive shifts,
hit testing and the recognition shapes above, with shell and prose negatives.
GPUI frame tests cover alternate-screen prose, sidebars and borders, a
coloured surface after a formula, and subscripted capitals in an
alternate-screen answer. These are unit and frame tests, not a final-GPU
screenshot of every TUI.

## Supersedes

The alternate-screen fixed-column contract asserted by the former
`alt_screen_renders_agent_math` frame test.

## Revisit when

A TUI is shown to align independent columns with single spaces, or terminal
snapshots start carrying the application's own layout regions.
