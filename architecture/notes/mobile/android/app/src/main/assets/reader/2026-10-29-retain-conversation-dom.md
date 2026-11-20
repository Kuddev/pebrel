# Retain unchanged conversation messages

## Status

Accepted for implementation; pending repository review.

## Context

The Android conversation reader reuses a WebView while transcripts update.
Replacing all of `main.innerHTML` on each update discarded selection, focus,
highlighted code, rendered formulas and diagrams even when messages were unchanged.
Late asynchronous enhancement callbacks could also restore an obsolete scroll position.

## Evidence

The browser regressions in `mobile/tools/test_reader_updates.mjs` reproduced lost
selection/focus, replaced KaTeX nodes and a stale scroll callback before the fix.
`ReaderMarkup.document` regenerates document-specific copy/link guard tokens;
retaining DOM without refreshing those tokens would invalidate native actions.

## Decision

Reconcile direct conversation children by message ID. Compare pristine source
signatures captured before enhancement, ignoring only the document guard token.
Retain unchanged nodes and refresh their original guarded anchors. Replace changed
messages, including messages whose link indexes have changed. Non-conversation
documents retain the full-replacement path.

Highlight only unprocessed code and render only unfinished diagrams/formulas.
Gate asynchronous scroll compensation by render revision and the reader's scroll
position. Do not follow output while text is selected. Preserve named tool expansion
and keep retained nested disclosures intact.

## Rejected alternatives

- Comparing enhanced DOM to source always reports changes after highlighting/math.
- Ignoring all link differences risks using the wrong native copy/link index.
- Retaining old guard tokens breaks the bridge's document identity check.
- Replacing the entire reader with a new framework adds scope without fixing the
  immediate ownership and scroll-lifetime errors.

## Consequences

Unchanged messages retain live DOM and avoid repeated expensive enhancements.
The update still parses incoming HTML and compares message sources; this is not
a wire-delta protocol or a measured end-to-end latency guarantee. Changed messages
may still lose selection within that message. Native transcript polling is unchanged.

## Validation

Run `node --test mobile/tools/test_reader_updates.mjs` with Playwright resolvable
through `NODE_PATH` and optionally `PEBREL_CHROMIUM` pointing to Chromium.
The tests cover selection/focus, bridge renumbering, real KaTeX/Mermaid node reuse,
history anchoring, bottom following, stale callbacks and both themes at phone widths.
Android WebView/device performance remains a separate validation surface.

## Supersedes

None.

## Revisit when

The wire protocol sends deltas, message identities change, or large histories make
source parsing/materialization a demonstrated bottleneck. Revisit within-message
patching if selection during streamed edits becomes a requirement.
