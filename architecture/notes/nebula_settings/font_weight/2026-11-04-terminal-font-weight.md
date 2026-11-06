# Terminal font weight preference

## Status
Proposed for review with issue #862, 2026-11-04.

## Context
[Issue #862](https://github.com/Kuddev/pebrel/issues/862) requests heavier terminal
text for readability on light backgrounds. The GPUI product previously built
normal/italic faces at 400 and bold faces at 700, independent of user preference.

## Evidence
`RuntimeSettings` owns additive preferences in the existing settings store.
`SettingsPane` saves them before broadcasting a settings change to open panes.
Terminal font construction and cell measurement share the typography adapter.
The existing custom-theme document can retain a weight without applying it;
that field does not define an existing global preference or precedence contract.

## Decision
Keep validation and regular/bold mapping in the zero-dependency
`TerminalFontWeight` model. Add one `font_weight` preference with the nine standard
weights. Missing, empty or invalid values preserve 400. Reset restores 400.
Normal and italic faces use the preference; ANSI bold adds 300, capped at 900.
This preserves the default 400/700 pair and prevents a heavier regular setting
from making ANSI bold lighter than regular text.

The GPUI adapter applies the setting to primary and explicit CJK faces at startup
and live reload. The existing CJK regular-for-bold option remains authoritative.
Cell measurement uses the selected regular weight. UI fonts are unaffected.
Persisting the setting reuses existing transactions and reload notifications;
failed writes restore the selector to the saved value and report the failure.

## Rejected alternatives
Changing only the selected font face leaves cell measurements and live sessions
inconsistent. Mapping all text to one weight loses ANSI emphasis. A second Lua
option or newly applying the dormant theme weight would introduce a separate
precedence change that is not needed for this request.

## Consequences
The saved format remains compatible: the new key is additive and older readers
can ignore it. Actual weight availability and synthetic styling remain platform
font-system decisions. At weight 900 the regular and bold numeric weights meet
at the maximum; the selector does not promise nine distinct installed faces.
The explicit legacy renderer does not gain a new font-weight control.
The current Linux backend selects actual faces rather than synthesizing heavier
strokes. Its bundled Maple Regular face alone can therefore look unchanged;
the settings hint directs users to a family with multiple weight variants.

## Validation
Shared settings tests cover defaults, invalid input, round trips, unrelated
settings preservation, reset and the bold mapping. Adapter and settings-view
tests cover font construction, reload and selector behavior; native font rendering
and visual acceptance are separate from those contracts.

## Supersedes
None.

## Revisit when
Independent bold weights, arbitrary variable-font weights, or a theme/Lua
precedence rule become an agreed user requirement.
