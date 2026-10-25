# Copilot testing scope

## Status

Accepted by the maintainer on 2026-10-01.

## Context

The maintainer requested that Copilot stop automatic code review and instead
reproduce reported bugs and produce regression tests. Unbounded speculative
review suggestions are not the desired use of this tool.

## Evidence

The previous workflow requested AI reviews after CI completion or when a PR became
ready. The repository also has an independently configured automatic Copilot review
ruleset. Disabling only one entry point is insufficient to establish the policy.

## Decision

Remove automatic events from the legacy review workflow and keep it disabled in
GitHub. Keep the automatic review ruleset disabled. Copilot instructions scope
assigned work to bug reproduction and tests, with exact-commit evidence and no
production changes unless explicitly requested. Assignment is maintainer-initiated;
new Issues do not trigger automatic agent work.

## Rejected alternatives

- Automatically assigning every Issue: feature requests, duplicates and reports
  lacking reproduction details are not interchangeable testing tasks.
- Replacing Copilot review with another automatic reviewer: does not meet the
  requested change in purpose.
- Deleting CI or weakening gates: testing assistance does not replace validation.

## Consequences

Copilot can prepare draft regression PRs and reproducible evidence for assigned
bugs. A test exposing an unresolved bug remains an explicitly failing draft until
fixed; maintainers continue to control production changes and merges. Instructions
define task scope, not a technical access-control boundary.

## Validation

The workflow contract includes a negative check for automatic event subscriptions.
Existing manual-request contract tests remain. GitHub workflow state and ruleset
state must be checked separately from repository YAML.

## Supersedes

[Automatic AI review after CI](2026-09-28-copilot-after-required-ci.md).

## Revisit when

The maintainer explicitly requests a different scope or a validated bug-triage
process with bounded agent assignments.
