# Allow the tmux name in the naming gate

## Status

Accepted by the maintainer on 2026-11-08.

## Context

The naming gate rejects listed product names in added text and commit messages.
tmux was on that list. Earlier changes kept it there and added narrow
per-file patterns, so that code which really uses tmux could still pass. These
patterns covered desktop resume endpoints, activity fixtures, mobile discovery,
history control and process fixtures. Prose, comments and commit subjects still
had to avoid the name.

## Evidence

The 2.4.0 follow-up chain (#882, folding #883–#887) fails `architecture-contracts`
on functional code and one commit message:

- the remote hook bridge reads `$TMUX` and sends a `tmux=` route field
  (`nebula_app/res/hooks/remote_tmux.py`, `remote_bridge.py`, `ai_hook/protocol.rs`);
- the Android SSH integration test runs real `tmux` commands;
- a merge commit message names the IME fix "in tmux".

Each of these would need another file-specific pattern, or a history rewrite of
an already reviewed merge chain. The checker already carried about 20 tmux-only
patterns and branches, with tests for each, and every new tmux integration
needed another governance change.

tmux is a compatibility target the product integrates with: hooks, session
discovery, recovery, the mobile client. It is not a competing terminal the policy needs to keep out of
product text.

## Decision

Remove tmux from `PROHIBITED_PATTERNS` and `PROHIBITED_EXAMPLES`. Delete the
patterns and branches that existed only to strip tmux references. Patterns
shared with other listed names keep their remaining alternatives. Tests that
used tmux to prove "the rest of the line is still checked" now use another
listed name. A new test fixes the counterexample: prose,
functional code and a commit message that name tmux are accepted, while text
appended to them is still checked.

## Rejected alternatives

- More per-file patterns for the new hook and Android files: does not cover the
  commit message, and the pattern set grows with every integration.
- Rewriting the merged chain's history to remove the word: changes reviewed merge
  commits and only defers the next conflict.

## Consequences

The tmux name may appear anywhere, including docs and commit messages. All other
listed names keep their existing rules and narrow allowances.

## Validation

`python3 -m unittest scripts.tests.test_prohibited_names`: 26 passed.
`check_prohibited_names.py range` over the #882 chain
(`merge/2.4.0-part3-20261104..merge/mobile-baseline-20261104`) reports no hits.

## Supersedes

The tmux-specific parts of
[remote control references](../naming/2026-10-22-remote-control-references.md),
[protocol and process fixtures](2026-11-01-protocol-and-process-fixtures.md),
[remote endpoint compatibility](../names/2026-10-25-remote-endpoint-compatibility.md),
[remote discovery compatibility](../policy/2026-09-30-remote-discovery-compatibility.md) and
[mobile integration references](../naming/2026-09-28-mobile-integration-references.md).

## Revisit when

tmux becomes a product the project competes with, or the maintainer wants to
restore the name to the prohibited list.
