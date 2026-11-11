# Faint remains a style fact until palette resolution

## Status

Implemented in the SGR 2 repair; current-head targeted validation is recorded with the change.

## Context

The terminal grid retained SGR 2, but the GPUI snapshot omitted it. Replacing a
color token with a pre-dimmed token looks smaller than carrying the attribute,
but discards information needed by the current palette and OSC color overrides.
The snapshot also swaps foreground/background for INVERSE before painting.

## Evidence

- `RenderSnapshot` already carries unresolved `Color` values and the terminal's
  color overrides; palette interpretation belongs to the frontend.
- With an OSC 4 override changing Red to blue and no explicit DimRed override,
  rewriting Red to DimRed before palette resolution returned theme dark red
  (`0x712b2b`) instead of dimmed blue (`0x0000a8`) in a production-source check.
- Applying DIM to the post-INVERSE foreground changed the wrong side of the
  color pair. The regression uses a non-black background so that this error is
  observable rather than hidden by black multiplied by the dim factor.

## Decision

The snapshot carries `dim` with the original foreground. INVERSE swaps its
ownership along with foreground/background, using `bg_dim` and `BgRun::dim` for
the resulting background. Text, box glyphs and OSC-link cells preserve the same
fact. Background runs only coalesce when both their color and dim state match.

`Palette::resolve_styled` resolves source overrides and explicit dim slots before
contrast correction. If correction bakes a replacement into `Color::Spec`, the
caller clears the carried dim flag to avoid dimming the result a second time.
Bold can still select a bold font; the dim-versus-brightening decision stays in
the palette rule rather than being independently reimplemented in each painter.

## Rejected alternatives

- Rewrite color tokens inside snapshot capture: source OSC overrides can be lost,
  and the terminal core would duplicate frontend palette policy.
- Re-read the live grid while painting: that breaks the owned snapshot boundary
  and can mix state from different revisions.
- Only dim the final swapped foreground: under INVERSE this dims the original
  background instead of the original foreground.
- Keep DIM after baking corrected RGB: that applies the factor twice.

## Consequences

No production dependency, thread, persistent setting or pane-read wire format
changes. Existing color-cache and surface-remap limits remain in force.
Capture still visits the visible grid once. Distinct dimmed backgrounds require
distinct runs, so DIM/INVERSE alternation can increase run count; its upper bound
remains one run per visible cell. This is not a process-memory or frame-rate claim.

The pre-existing bold-brightening behavior under INVERSE and the existing text
contrast policy are separate from this repair.

## Validation

Use the actual Term/snapshot tests for DIM, reset and INVERSE; palette/color tests
for indexed, named, truecolor, OSC overrides and single dimming; and the existing
ligature style-boundary test. Native product checks remain separate from these
color calculations, which do not constitute GPU screenshot acceptance.

## Supersedes

None.

## Revisit when

A future renderer changes the snapshot style contract or the shared palette
policy. Preserve source-color identity and original-foreground ownership when
replacing these adapters during legacy-shell retirement.
