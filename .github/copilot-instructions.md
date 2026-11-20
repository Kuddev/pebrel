# Copilot scope: bug reproduction and regression testing

Copilot is used for testing reported bugs, not automatic pull-request review.
Work only on the Issue or test task explicitly assigned by a maintainer. Do not
scan unrelated Issues, request reviews, approve/merge PRs, or publish releases.

## Reproduce first

- Read the assigned report and relevant comments. Record the reported version,
  platform, configuration, expected behavior and actual behavior.
- Reproduce on the stated version when available and on the current target commit.
  Report the exact commit, commands, environment and observed output. Distinguish
  a confirmed failure, an already-fixed report and a failure to reproduce.
- When information or platform access is missing, state precisely what is missing.
  Do not invent results, infer a UI result from compilation, or call a theoretical
  edge case a confirmed defect.
- For an unclear report, reply to the reporter on the assigned Issue with concise
  questions about the missing reproduction details. Ask only for information that
  blocks testing: a minimal input/command and steps, relevant version/platform or
  settings, and sanitized logs. Use the reporter's language and do not request
  information already present in the report.
- If the current code already fixes the bug or existing tests cover it, report
  the verified results instead of adding duplicate tests.

## Deliver focused test evidence

- Prefer a minimal regression in the existing test harness exercising the real
  production path. The assertion must expose the reported behavior, not mirror an
  implementation or merely assert that code does not panic.
- Demonstrate that the regression fails on the affected code for the reported
  reason. If a fix is already available, run the same test against it and record
  the passing result. Keep an unresolved reproduction PR in draft and explicitly
  identify the expected failing test; never merge a known-red suite.
- Keep changes to tests and necessary fixtures by default. Do not add speculative
  guards, retries, fallback layers, broad refactors, dependencies or production
  fixes unless the maintainer explicitly expands the task.
- Run the relevant tests and report what actually ran, failed or could not run.
  Include a minimal reproducer, results and the remaining uncertainty in the PR.
  Keep credentials, personal settings and private paths out of public evidence.

Follow `AGENTS.md`, the nearest module instructions, `CONTRIBUTING.md`,
`docs/architecture.md` and `docs/project-constraints.md`. Reuse existing contracts;
never weaken required checks, raise budgets, delete coverage or hide failures.
Deterministic CI and maintainer review remain the merge authority.
