# Stable no-op hook reconciliation

## Status
Maintainer fix for a demonstrated configuration-churn path associated with #454.

## Context
The application periodically checks its managed hooks and repairs outdated entries.
Reconciliation previously removed all recorded groups and appended the desired
groups, then serialized the complete JSON document even when managed content was
unchanged. User entries added after a managed group moved before it, and external
formatting was lost. This made an already-current installation look outdated.

## Evidence
The new adapter regression installs managed hooks, appends an unrelated user hook,
adds opaque user metadata and writes compact JSON with CRLF. On the original code,
`current_for_mode` incorrectly returns false. This is an application-side file
change; it does not establish how every external hook editor persists its trust.

## Decision
When the recorded and desired groups are equal and each managed group occurs
exactly once, return the original bytes instead of rebuilding the document.
Retain hook positions, unknown metadata and external formatting. The existing
byte comparison then performs no file write or replacement.

Do not bypass ownership checks: an edited group does not qualify for this path.
Duplicates still reach the existing repair logic. Command updates and removal
continue through the original merge path. No trust or approval record is created,
changed or inferred by this fix.

## Rejected alternatives
- Rewriting equivalent JSON keeps triggering file observers and external review
  mechanisms despite having no managed content to change.
- Copying or synthesizing trust flags would assume an external schema and cross
  the boundary between installing a hook and approving its execution.
- Ignoring edited groups or ownership markers would silently adopt user changes.
- Comparing only helper names would miss meaningful command or timeout changes.

## Consequences
The cold reconciliation path avoids needless I/O without adding a persisted field,
worker or polling loop. Its equality check remains bounded by the existing hook
configuration limit. Real changes can still require external review; this is not
a blanket promise that every client will stop prompting.

## Validation
The new regression failed before the change and passes afterward. Six local
adapter tests and four shared installation-policy tests pass, including literal
Windows command execution, upgrade/removal, repeated toggles, opt-out, edited-hook
preservation and duplicate repair. Tests use temporary configurations rather than
the user's live hook files. The report's exact editor/version still needs a
matching end-to-end confirmation before closing #454.

## Supersedes
None.

## Revisit when
Hook schemas, ownership markers or external editor behavior change. Preserve
byte stability for genuine no-ops while keeping real command changes reviewable.
