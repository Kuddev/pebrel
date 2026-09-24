# Keep the size decision independent of optional PR writes

## Status

Implemented for review.

## Context

Fork pull requests have a read-only token. The required size check counted files,
then tried to add a size label before reporting its decision. An ordinary small
contribution therefore failed with HTTP 403 instead of reporting its actual size.

## Evidence

PR 274 at `98fad00b01da9dd089581e4d4f66a251bfc1404d` changes 127 Rust source lines.
Run 35893377849 had PullRequests-read permission and failed at POST issue labels
while trying to add the ordinary `size/M` classification. Its native tests passed.

## Decision

Compute and publish the size result before optional writes. The existing 1500-line
limit, ignored documentation/assets and maintainer exemption semantics remain the
same. An oversized fork fails immediately. Forks report their result in the job
summary and do not attempt labels or comments with a read-only token. Same-repository
pull requests retain their existing labels and oversized-change guidance.

Tests execute the actual inline workflow script with fake APIs, so there is one
implementation of the counting and fork boundary. Node is already available on
the supported hosted images; the tests require it and use no npm dependencies.

## Rejected alternatives

Broadening fork token access or executing fork code in a privileged PR event is
unnecessary. Suppressing the entire size check on forks would waive its invariant.
A separate privileged labeling service adds another trust boundary and is not
needed to validate source size.

## Consequences

Fork contributors receive a genuine size-check result without write access.
Automatic size labels remain available for same-repository contributions;
maintainers may apply the ordinary size label to a fork. Such a label is not a
budget exemption, and the source count is still recomputed by the required check.

## Validation

The workflow-script tests cover a small fork, oversized fork, exact limit,
existing maintainer exemption, deleted-fork metadata and same-repository guidance.
No test calls the real GitHub write APIs. Actionlint validates the workflow.

## Supersedes

None.

## Revisit when

A metadata-only labeling service is justified, or the source-size policy changes
through a separately reviewed rule update.
