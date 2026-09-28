# Review guidance

Review concrete regressions and missing boundary cases in the changed code. Explain
an actionable finding with the trigger, affected behavior, and file/line evidence.
Distinguish confirmed defects from questions; avoid speculative redesigns and style
comments already covered by formatting. Do not infer success from a passing build.

Follow `AGENTS.md` and the closest module instructions, `CONTRIBUTING.md`,
`docs/architecture.md`, and `docs/project-constraints.md`. Treat them as the
source of project policy rather than inventing a second rule set. Check ownership,
dependency direction, cancellation/lifetime, persistence compatibility, and tests
appropriate to the change. Consult `docs/internationalization.md` for UI text and
`packaging/AGENTS.md` for release changes. Verify performance claims against their
stated workload and separate platform tests from visual acceptance.

CI remains the deterministic merge gate. Do not recommend weakening required
checks, widening budgets, deleting tests, or treating this automated review as
maintainer approval. For CI changes, look for missing-platform, skipped-test,
stale-commit, cache-poisoning, fork-permission, and failure-propagation regressions.
