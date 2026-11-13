# Batch verified fixes in stable releases

## Status

Accepted by the maintainer on 2026-11-13. See [packaging rules](../../../packaging/AGENTS.md).

## Context

Stable updates ship on multiple platforms, and each version asks users to update again.

## Evidence

[2.5.1](https://github.com/Kuddev/pebrel/releases/tag/v2.5.1),
[2.5.2](https://github.com/Kuddev/pebrel/releases/tag/v2.5.2), and
[2.5.3](https://github.com/Kuddev/pebrel/releases/tag/v2.5.3) were published within
three hours. Each addressed one narrow user workflow.

## Decision

Before bumping the stable version, review at least ten distinct user-facing fixes
against the latest published stable source. Record each defect, the commit that
fixes it, and its verification. Count one defect once even when it needs several
commits or spans multiple platforms. Keep the normal package and publication checks.

## Rejected alternatives

- Publish a stable update for each fix: it repeats the release and upgrade work.
- Count commits or changelog lines automatically: those do not establish which
  defects are distinct, newly fixed, or verified.

## Consequences

Small changes can be developed, reviewed, and tested as they land. Stable versions
advance when the next verified batch is ready.

## Validation

The maintainer reviews the candidate against the previous release and checks each
entry's evidence before changing the version. The count is a review rule.

## Supersedes

None.

## Revisit when

The maintainer changes the stable release policy.
