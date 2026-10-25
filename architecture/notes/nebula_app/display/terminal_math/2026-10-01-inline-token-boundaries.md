# Inline formula boundaries and mathematical atoms

## Status

Implemented; release acceptance is recorded separately.

## Context

Issue #420 reports intermittent inline formula failures, especially single
numbers, some single-letter symbols and formulas wrapped by an agent's output
formatter rather than by the terminal. Recognition happens before compilation;
an accepted oversized source can therefore hide later valid formulas even when
the oversized source subsequently fails compilation.

## Evidence

The real scanner regression for `$E$`, `$0$`, `$2.71828$`, a quadratic expression
and `\pm` returned one source containing the intervening dollar delimiters and
Chinese prose instead of five formulas. The currency filter rejected the early
atoms, and the closing search then borrowed later delimiters.

The hard-wrapped integral regression returned no formulas. The old search only
crossed rows marked `WRAPLINE`, which an output formatter's explicit newline
does not set. The prior fixtures explicitly encoded both the numeric rejection
and the unconditional hard-newline rejection, so those contracts need revision.

## Decision

Accept finite numeric literals and individual alphabetic symbols inside explicit
formula delimiters. Retain rejection of unpaired prices, currency-code phrases,
shell identifiers, paths and ordinary prose. A paired `$5$` is a numeric formula;
ordinary currency uses `$5` rather than a closing math delimiter.

Stop at the first single-dollar closing candidate. If its boundary or source is
invalid, resume the outer cell scan instead of expanding the same source through
later formulas. A dollar followed by an ASCII identifier character is not borrowed
as a closer from the next shell variable or formula.

Permit mathematical hard-wrap fragments within the existing source-byte limit
and at most eight hard row transitions. Every completed hard row and the closing
hard row must independently contain a mathematical fragment. Blank rows and prose
stop the search. Soft wrapping retains its normal behavior. Whitespace immediately
inside explicit inline delimiters is allowed; shell commands still need real
mathematical evidence before they can be recognized.

Recognize polynomial sums in relation operands and explicit bracketed intervals
using the existing compact operand rules. Compilation, bitmap caching, source
cell ownership, selection projection and terminal input remain unchanged.

## Rejected alternatives

- Accept any dollar-delimited text: shell variables and prose are also accepted
  by the underlying parser and would become visible mathematical overlays.
- Keep searching after an invalid closed span: reproduces the adjacent-formula
  loss demonstrated in the issue fixture.
- Cross every newline until a dollar appears: merges unrelated output paragraphs.
- Change terminal grid contents or run an agent to repair output: recognition
  should consume existing terminal facts without owning input or execution.

## Consequences

Paired numeric spans now render as formulas rather than being treated as prices.
Arbitrary prose, unbounded multi-paragraph math and every possible terminal
formatter are not inferred. The scanner remains bounded and performs no I/O.

## Validation

The issue regressions failed against the previous scanner. Existing scanner
fixtures retain shell, currency, escaped-delimiter, display-block, source-cell
ownership and bounded-search coverage. The terminal overlay fixture also checks
that recognized issue examples reach asynchronously prepared bitmaps. Unit and
bitmap tests are distinct from native terminal/PTY visual acceptance.

## Supersedes

The old paired-numeric and unconditional inline hard-newline rejection fixtures.
Other delimiter, rendering and terminal ownership contracts remain unchanged.

## Revisit when

A reproducible formatter output requires a different continuation boundary, or
new evidence shows an admitted fragment consumes non-mathematical terminal text.
