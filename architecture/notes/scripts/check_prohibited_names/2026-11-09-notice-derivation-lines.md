# Allow license derivation lines in third-party notices

## Status

Accepted with the maintainer-requested Pebrel naming migration, 2026-11-09.

## Context

`THIRD-PARTY-NOTICES` names the source path of code adapted from an Apache-2.0
project and states that it "derives from" that project, as the license requires.
Renaming the source directory changes that line.

## Evidence

After moving `nebula_app` to `pebrel_app`, the range check reported only the
notice line for `pebrel_app/src/agent_detection`. The existing attribution
patterns cover copyright and "third-party notice" phrasing, not a derivation
line, and any edit to the path rewrites the line that names the project.

## Decision

Only in `THIRD-PARTY-NOTICES`, remove a trailing `derives from <Name>` before the
name check. The rest of the line is still checked, and the same phrase in any
other file is still rejected.

## Rejected alternatives

- Exempt the notices file: comparisons or promotion added there would pass.
- Keep the old path in the notice: it would point at a directory that no
  longer exists.
- Drop the project name: the license requires the attribution.

## Consequences

Notice lines can follow source moves. A comparison or another name on the same
line still fails.

## Validation

`test_notice_derivation_is_allowed_only_in_third_party_notices` covers the
allowed line, the same line in another file, and appended text before and after
the derivation.

## Supersedes

None.

## Revisit when

Notices move to per-component license files.
