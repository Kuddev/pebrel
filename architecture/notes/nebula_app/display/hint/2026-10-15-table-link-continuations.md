# Terminal table URL continuation boundaries

## Status

Proposed for 2.2.1.

## Context

Issue #620 shows a web URL split inside one bordered table cell. The browser
receives only the first fragment and returns 404. Terminal soft wraps already
belong to one logical line, but program-authored table rows can contain hard
newlines, padding and other columns.

## Evidence

The issue's two screenshots show `.../nebula_ap` followed by
`p/src/config/ui_config.rs` in the same cell, with a blank identity column on
the continuation and a horizontal record separator. They do not include a
terminal byte recording or grid flags. PR #635 explicitly excludes table
record continuation. [Hint tests](../../../../../nebula_app/src/display/hint.rs)
use a fixture matching this visible structure; it is not a claim to reproduce
the exact captured bytes.

## Decision

Retain normal logical-line regex matching and OSC 8 target ownership. For bare
HTTP(S) links in bordered tables, follow the same cell boundary columns only
when the previous fragment fills that cell, the next fragment starts at the
same content column and the identity cells on that continuation are blank.
Stop at record borders, changed columns, whitespace inside the token or a
new HTTP(S) target. Other columns may independently wrap their explanations.

Keep per-row fragment ranges for decoration. Revalidate the table structure
and first regex match at activation, then concatenate only cell contents for
the target. Traversal is bounded to 32 rows and 64 KiB; an identified
continuation exceeding that budget does not yield a clickable prefix.
Likewise, the existing regex search budget must not turn an artificial
100-row boundary into a complete URL match.

## Rejected alternatives

- Joining every hard newline can merge independent URLs, prompts and text.
- Using the shortened hover preview loses the full target; it is presentation
  only and was not the source of this incident.
- Adding guessed suffixes when neither grid nor OSC 8 contains them invents
  a destination.

## Consequences

The reported table shape supports Ctrl-click on either fragment. Plain hard
newlines remain separate. Text-only table layouts retain ambiguity when an
author deliberately breaks one cell into URL-shaped tokens; OSC 8 supplies
an explicit target and keeps precedence. This is not a generic table parser
or a guarantee for arbitrary borderless terminal layouts.

## Validation

Regression fixtures cover both fragments, record and new-URL boundaries,
redraw invalidation, normal soft wraps, shortened OSC 8 labels and artificial
search limits. Existing shell-path and hyperlink cases remain covered.

## Supersedes

None.

## Revisit when

A real output recording demonstrates another structural layout, or the
producer supplies explicit OSC 8 targets for all wrapped links.
