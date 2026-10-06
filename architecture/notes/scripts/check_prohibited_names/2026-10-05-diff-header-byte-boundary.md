# Diff metadata and source decoding boundaries

## Status

Proposed maintainer repair for the reproduced PR #351 check failure.

## Context

The naming checker must scan added source and pending commit messages, including
text added and later removed. It decoded an entire Git diff as UTF-8 before
finding source lines.

## Evidence

Git 2.55.0 emitted a combined hunk title ending in bytes `e8 b0`, cutting the final
codepoint of a Chinese context label. Both document revisions are valid UTF-8.
The failing input is reproducible with `git show --format= --cc --no-ext-diff
--unified=0 78b20d2c -- docs/runtime-control-api.md`. The title is metadata, not an
added source line; decoding it rejected a legitimate edit before scanning it.

## Decision

Read physical diff lines and ASCII hunk/addition prefixes as bytes. Decode only
the added source payload, still using strict UTF-8. Staged and commit scans share
that implementation. File headers are recognized by their pre-hunk position,
so added source beginning with multiple plus signs remains subject to checking.

## Rejected alternatives

- Replacement decoding would hide invalid added source bytes.
- Ignoring files, commits or the failing check would weaken the actual policy.
- Editing legitimate document text merely to change Git's context truncation
  would conceal the parser defect.

## Consequences

Patterns, exemptions, commit-message/path decoding and whole-history scope are
unchanged. Malformed added UTF-8 still fails. There is no product runtime change.

## Validation

Focused regressions cover regular and combined truncated headers, prohibited
source after those headers, plus-prefixed source and invalid added bytes. Run the
existing naming-check suite and the original failing range before integration.

## Supersedes

None; repairs metadata parsing without changing the naming policy.

## Revisit when

Git's diff output format or the checker's source selection changes.
