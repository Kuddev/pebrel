# Commercial promotion review boundary

## Status

Accepted by the maintainer on 2026-09-30; enforcement remains a review task.

## Context

A provider-preset contribution was initially approved because it was small and
from a first-time contributor, without first establishing the project's need.
The maintainer identified it as unwanted promotion and required its exclusion.

## Evidence

- [PR #346](https://github.com/Kuddev/pebrel/pull/346) is closed, unmerged, and its
  maintainer approval is dismissed. The remaining maintainer review states the
  prohibition rather than endorsing the provider.
- The maintainer explicitly confirmed that [PR #241](https://github.com/Kuddev/pebrel/pull/241)
  is approved sponsorship. Brand mentions alone therefore do not establish a violation.
- The maintainer requested direct closure of verified unauthorized promotional
  PRs/Issues, and reserved README for product information and a business contact.

## Decision

Keep the authoritative scope, review procedure, and email in
[CONTRIBUTING.md](../../../CONTRIBUTING.md#commercial-promotion-policy).
Agent instructions, review guidance, and submission templates link to it.
Close verified unauthorized promotional submissions without a second approval.
Preserve scoped sponsorship and necessary compatibility/attribution content.

## Rejected alternatives

- Small changes, first contributions, or green CI as authorization: these assess
  neither user need nor promotional intent/content.
- A keyword/domain blacklist: it would also match legitimate integrations,
  attribution, and explicitly approved sponsorship.
- Closing an innocent issue after an unrelated spam comment: this would allow
  outsiders to suppress legitimate reports. Moderate the offending comment.
- Repeating the full rule in README and every template: copies drift; README
  keeps only its business-cooperation and sponsorship contact wording.

## Consequences

Review needs evidence of content and authorization, not guesses about an author.
Mistaken approvals must be withdrawn and pending integrations corrected.
This adds no bot, workflow, automatic scanning gate, or server-side setting.

## Validation

Acceptance examples: unauthorized promotional preset → close; explicitly approved
sponsorship → preserve within scope; necessary upstream notice → preserve;
unrelated spam comment → moderate the comment, keep the legitimate submission.
Check UTF-8, policy anchors, and YAML parsing; preserve existing form fields.
No application build is needed for this documentation-only policy change.

## Supersedes

None.

## Revisit when

The maintainer changes sponsorship scope, business contact, or moderation policy.
Update the authority and linked summaries together rather than adding copies.
