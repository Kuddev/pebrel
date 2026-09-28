# AI review after required CI, with an allowance fallback

## Status

Implemented; activation requires the default-branch workflow, a dedicated owner
credential, and verified CodeRabbit installation before enabling its fallback.

## Context

Reviews of every push spend allowance on drafts, failed builds, and changes that
already have an initial review. Native branch rules do not express the requested
combination of authorship, readiness, CI, review history, and allowance routing.

## Evidence

GitHub exposes effective required checks through the branch rules API and results
for a specific commit. The reviewer API accepts `copilot-pull-request-reviewer[bot]`.
A user-authenticated request produced a review, whereas an Actions-token probe
returned success without a Copilot run or review. API acceptance alone therefore
cannot establish delivery. A user credential is required for activation testing.

The authenticated Copilot client endpoint exposes a timestamped allowance snapshot,
including remaining credits and whether paid overage is permitted. This endpoint
is not a stable public billing contract; response drift must stop automation.

## Decision

The metadata-only [workflow](../../../../.github/workflows/copilot-review.yml)
requests one review for an open, non-draft PR by a human other than the repository
owner, after every required check on its current head succeeds. It reads effective
server-side branch rules rather than copying their check list. No required-check
policy means no automatic review.

Completed CI workflows and draft-to-ready transitions trigger eligibility scans.
A repository-wide concurrency group serializes requests; scanning open PRs covers
events replaced in GitHub's pending slot. Dispatch evaluates one existing PR with
the same guards. Previous Copilot or CodeRabbit reviews prevent automatic repeats.

The workflow's built-in token reads PR/CI metadata and writes claim labels.
`COPILOT_REVIEW_TOKEN` must belong to the repository owner and is used only for
account allowance and user-attributed review requests/comments; it needs no Checks,
Actions or Contents permission for metadata reads. A positive Copilot
allowance selects Copilot. Zero allowance selects CodeRabbit only when
`CODERABBIT_FALLBACK_ENABLED=true`, after installation has been verified. Missing,
negative, stale, unrecognized, or unreadable allowance data stops requests; an
ordinary permission/network error never implies exhaustion. Paid overage must be
disabled. Credits are not a fixed number of reviews, and consumption reporting can
lag; this router does not establish a provider-side spending cap.

A persistent `ai-review-requested` label is written before requesting either bot;
older `copilot-review-requested` claims remain recognized. On uncertain delivery or
failure, keep the claim and require manual inspection before retrying. No automatic
retry loops or default dual review. CodeRabbit's own automatic and incremental
reviews are disabled in the repository config; the fallback uses an explicit
`@coderabbitai review` request. Instructions prioritize evidence-backed defects.

The workflow never checks out PR code, runs artifacts, interpolates PR text into
commands, approves a PR, or merges changes. Human review and required CI remain
unchanged. The dedicated credential is a repository secret, never checked in;
provision and validate it separately from the workflow's source.

## Rejected alternatives

- Branch-wide rules or every-push reviews: cannot enforce the requested policy.
- Executing fork code with privileged credentials: unnecessary for metadata routing.
- Treating any Copilot error as exhausted credit: hides outages and permission bugs.
- In-memory deduplication or uncertain retries: can request two paid reviews.
- Hard-coded check names: drift from the effective merge requirements.
- Copying a developer's broad CLI credential into Actions without explicit setup.

## Consequences

Maintainers request subsequent reviews manually. A push can race the final API
call; rechecking head/draft/base/claim immediately before submission narrows but
cannot eliminate that race. An accepted request followed by asynchronous service
failure remains for manual inspection rather than risking duplicate reviews.
Unprotected target branches and legacy status-only checks require manual review.
This is supplemental automation, not another mandatory merge check.

## Validation

Offline fixtures execute the actual script: successful external PR; owner, bot,
draft and closed exclusions; missing/failed/skipped checks; newer pending attempts;
wrong app/head; racing changes; review history; claims; policy/quota failures;
positive versus exhausted allowance; disabled fallback; and no double request.
The required lint job runs the contract before scheduling native runners.
Actual owner-secret delivery and CodeRabbit response must be verified on GitHub
before claiming end-to-end activation. Do not consume the real allowance merely
to force exhaustion; the zero-credit routing boundary is covered with fixtures.

## Supersedes

None.

## Revisit when

GitHub exposes a stable allowance API or equivalent native routing, supports atomic
SHA-bound review requests, required-check providers change, or ownership changes.
