# Prohibited-name range checks

## Status

Implemented for review. The existing local hook modes remain unchanged; the public CI entry point is `range --base BASE --head HEAD`.

## Context

The repository already used an ignored `scripts/check_prohibited_names.py` in staged, commit-message, and pre-push hooks. The policy must also run in the existing `architecture-contracts` check for pull requests, merge groups, and pushes to `main`, without rescanning old release history or making legitimate attribution and protocol compatibility impossible.

## Evidence

- `.github/workflows/architecture.yml` already runs on `pull_request`, `merge_group`, and `push` to `main`, with a full checkout.
- The local checker already owns the prohibited-name list and hook behavior; duplicating the list in workflow YAML would create a second policy source.
- Inline copyright/third-party attribution can contain a project name, while Cargo git/source references and protocol/external-format identifiers include key-reporting constants, multiplexer escape sequences, and theme-format variants.

- macOS run 35950152526 rejected a malformed UTF-8 filename before the checker
  could run. The fixture now writes a Git tree directly, preserving the same
  invalid-path regression without depending on filesystem filename support.

## Decision

- Publish the existing checker as the single policy implementation and add a fail-closed range mode.
- Resolve both revisions, compute their `merge-base`, scan ordinary commits against their first parent and merge commits with a combined diff, and scan every commit message in the resulting range. Missing, zero, unreadable, or unrelated revisions return failure. Invalid UTF-8 in paths, patches or messages fails explicitly rather than substituting another path or text.
- Keep only the checker, its contract tests and the intentional negative comparison fixture as exact path exceptions. Strip only reviewed inline attribution, Cargo git/source references, and protocol compatibility identifiers before applying the existing patterns.
- Run the checker as a step inside `architecture-contracts` and run its unit tests with the other guardrail tests. No new required check or server-side ruleset change is introduced.

## Rejected alternatives

- Scanning the whole repository would fail on historical, already-reviewed attribution and compatibility names.
- Using only `git diff base..head` would miss a prohibited name added and removed by separate commits; range mode reuses the hook's per-commit added-line scan. Comparing every merge to its first parent would replay second-parent main changes, so merge commits use combined diff instead.
- Treating an unknown base as an empty range would make an unavailable GitHub event input look green; the workflow derives only a known first-push parent and otherwise fails. A normal PR that has diverged from main uses its merge-base.
- Adding a broad `docs/`, `src/`, or product-directory exemption would hide ordinary competitor comparisons, so exemptions remain exact and line-scoped.

## Consequences

New PR/merge-group/main-push text and commit subjects are checked by the existing required job. A legitimate inline attribution, real dependency source, or supported protocol identifier remains usable. Adding another legal/protocol/dependency spelling requires a narrow fixture and policy update rather than silently bypassing the scan.

## Validation

`scripts/tests/test_prohibited_names.py` covers ordinary comparison failure, inline attribution positive/negative paths, protocol identifiers, Cargo dependency references, GitHub CLI text, range text, range commit messages, diverged related bases, unrelated/missing bases, old-history exclusion, added-then-deleted text, and merge-resolution additions. The checker also covers malformed UTF-8 paths/text and normal Unicode/control-character paths; the malformed-path fixture constructs Git tree objects without checking them out, so it also runs on filesystems that reject those bytes.

## Supersedes

None.

## Revisit when

GitHub changes the event payload fields used for base/head selection, the repository adopts a different merge queue event, or a new legal/protocol/dependency contract requires a narrowly documented exception.
