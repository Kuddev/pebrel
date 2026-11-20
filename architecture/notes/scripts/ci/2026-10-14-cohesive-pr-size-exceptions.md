# Keep complete modules reviewable within a bounded PR-size exception

## Status

Accepted maintainer policy: organize PRs around complete, independently verifiable
feature modules. Individual reviewed exceptions may exceed the default 1500-line
threshold, but no PR may exceed 5000 counted source lines.

## Context

The existing PR-size workflow already accepted a maintainer-applied `size-exempt`
label. Its contribution guide instead told every oversized PR to split, while the
label description mentioned integration branches only. The implementation also
had no upper bound once the label was present.

## Evidence

The existing workflow tests execute the actual inline action. A 1501-line fork
with `size-exempt` passed despite the guide's unconditional splitting language.
New boundary assertions demonstrate that the old action also accepted 5001 lines
with the label, while its ordinary rejection still suggested an unlimited waiver.
Both fork and same-repository paths reproduce this mismatch.

GitHub's [file-list endpoint](https://docs.github.com/en/rest/pulls/pulls#list-pull-requests-files)
documents a maximum of 3000 returned files. A partial list cannot prove compliance
with a 5000-source-line ceiling, especially when ignored files precede source files.

## Decision

Keep one counting implementation and the existing additions-plus-deletions
measure, including its documented documentation, lockfile and asset exclusions.
Keep 1500 as the default threshold. Maintainers may approve a specific larger PR
with `size-exempt` and a short rationale when retaining the module supports review
and verification. Check the 5000-line ceiling before applying that exception.

Compare the listed file count with the PR metadata before counting lines. Missing
or incomplete file metadata fails explicitly; a label cannot make an incomplete
count trustworthy. No additional file-count limit is hard-coded in the action.

The summary reports the actual count, both thresholds and label presence.
Guidance favors cohesive module boundaries over arbitrary line-count slicing.
The label is not an exemption from tests, review, dependency rules or physical
per-file budgets. No additional privileged workflow or automatic waiver is added.

## Rejected alternatives

- Require every complete module to fit 1500 lines: this can encourage mechanical
  splitting and separate necessary behavior from its verification.
- Let the label bypass every size: a maintainer-approved exception still needs
  the explicit 5000-line ceiling.
- Raise the default for every PR: individual exceptions remain review decisions.
- Change file budgets or remove tests to fit: PR churn and physical file limits
  protect different contracts.

## Consequences

The existing S/M/L/XL labels continue to report size even for accepted exceptions.
Fork tokens retain read-only behavior; counting and failure happen before optional
same-repository label/comment writes. Existing label/unlabel events recompute the
decision. Other required checks and server-side protection settings are unchanged.

## Validation

The existing action-script suite covers the default 1500 boundary, a rejected
1501-line PR, accepted 1501/5000-line exceptions, and rejected 5001-line PRs with
and without the label on both fork and same-repository paths. The new assertions
fail against the previous action, then pass after the ceiling is implemented.
Missing or truncated file metadata fails with and without the exemption label.
This does not claim a live oversized negative PR was opened.

## Supersedes

The unbounded exception scope referenced by
[fork size checking](2026-09-24-fork-size-check.md). Its counting and fork-token
ownership rationale remains applicable.

## Revisit when

The maintainer changes the review-size policy with a concrete counterexample and
boundary tests, or the source-counting contract changes.
