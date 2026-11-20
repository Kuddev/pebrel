# Publish verified stable batches

## Status

Accepted for the stable release series beginning with 2.6.0.

## Context

The maintainer wants to publish each group of roughly ten independent non-UI fixes while work continues on the remaining issues. The stable line and development branch can contain different sets of changes.

## Evidence

A single resource collector was recorded as two merged pull requests, #559 and #1023. The release candidate also contained fixes developed and verified directly against the prior stable source. A PR count would miscount both cases.

## Decision

Build each batch on the previous public stable source. Maintain the next version on a branch named `release/auto-X.Y.Z`. While reviewing each fix, backport its code, run the regression and record its stable ID, full release-branch commit and verification in `packaging/release-batch.json`. Keep the package versions, bilingual release notes and changelog current on that branch. After 2.6.0 the default increment is a patch unless the maintainer selects a different version.

For example, an entry for a completed fix uses this structure. Use the actual commit from the release branch.

```json
{
  "base": "v2.6.0",
  "fixes": [
    {
      "id": "issue-123",
      "commit": "<40-character release-branch commit>",
      "verification": "<regression and measured result, or its CI job URL>"
    }
  ]
}
```

Review establishes that a change fixes a distinct defect, meets the batch scope and has passed its acceptance case. Changes for one defect share one entry. At ten verified entries the branch push starts the existing stable build and publication workflow on that exact commit. The workflow checks the current public baseline again before publishing. After publication, the workflow opens the next patch branch from the public tag with an empty manifest. The first reviewed backport on that branch supplies the new package version and release notes. A retry preserves work on an existing branch descended from the tag.

## Rejected alternatives

Counting PRs, commits or paired English and Chinese changelog lines does not establish distinct verified fixes. Publishing a snapshot of the development branch could bring unrelated UI changes into the release.

## Consequences

Maintainers can continue merging work while a release runs. Each eligible batch uses the normal native matrix, package evidence, release checksums and publication checks. A failed run can be retried on the same commit or on a corrected batch commit.

## Validation

The batch tests exercise nine and ten fixes, duplicate entries, missing evidence, previous-release commits, disconnected history and an already published version. Stable workflow tests cover automatic and manual publication guards. Bare-remote tests exercise two successive patch releases, retry after another fix arrives, an unrelated existing branch, an unpublished commit and a dirty checkout.

## Supersedes

Extends the previously reviewed rule to batch ten verified fixes per stable version. The maintainer's review still establishes which defects qualify.

## Revisit when

The stable release line, batch size or accepted scope changes.
