# Graphics protocol and real process fixtures in naming checks

## Status

Prepared for maintainer review under the task's explicit instruction to retain
necessary technical terms. This is a scoped governance correction, separate from
the release candidate and its behavior fixes.

## Context

The naming gate preserves compatibility interfaces while rejecting incidental
product mentions. Release documentation needs the proper graphics protocol name.
Terminal activity regressions must use the real executable identity consumed by
the implementation, including a terminal multiplexer with an agent inside it.

## Evidence

PR #849 failed the range check in run `41242096322`. The reported additions mix
two legitimate interfaces with incidental comments, a hook fixture identifier,
and a commit-message mention. Treating every reported line alike would either
break meaningful tests or admit unrelated prose.

Two new regression methods reproduce four failed positive subcases against the
previous checker: English and Chinese protocol names and two executable fixtures.
The incidental prose and fixture label deliberately remain negative examples.

## Decision

Recognize the complete English and Chinese graphics protocol names alongside
the existing keyboard protocol forms. In the owning activity test source only,
recognize the actual process assignment and the two-program fixture loop.
Remove only those exact fragments, then scan all remaining text as before.

Do not add a file, directory, commit or accepted subject to exemptions. Keep the
per-commit range scan, including added-then-deleted content. Commit messages,
comments, arbitrary strings and test labels retain their existing restrictions;
the original failing candidate range is not expected to pass unchanged.

## Rejected alternatives

- Encode, concatenate or rename the real executable to evade the checker: this
  conceals the compatibility interface and weakens the regression.
- Admit every literal or comment in test files: this hides unrelated comparisons
  and promotional URLs beside valid fixtures.
- Exempt historical commits or switch to a final-tree-only scan: this loses the
  existing contract for content added and later removed.
- Broaden commit-message exceptions: process fixtures do not require that change.

## Consequences

Real process identity and proper protocol names remain readable. Incidental
comments, hook labels and commit prose must be cleaned in a normal replacement
branch; published branches do not need to be force-pushed. Runtime behavior and
the prohibited product-name list are unchanged.

## Validation

The naming and architecture-governance suites pass: 43 tests. Positive fixtures
are paired with wrong-path, arbitrary-string, appended-comparison and appended-URL
negative cases. The old checker fails all four new positive subcases. Existing
staged, message, push/range, Unicode and merge-resolution coverage remains intact.

## Supersedes

None. Extends exact compatibility recognition while retaining the editorial
boundary in `2026-10-22-remote-control-references.md`.

## Revisit when

The protocol terminology or the owning process-identity fixtures change. Supply
both a real false positive and out-of-scope counterexamples before extending it.
