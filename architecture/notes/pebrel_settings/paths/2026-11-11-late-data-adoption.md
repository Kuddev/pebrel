# Reuse non-replacing publication for late legacy data adoption

## Status

Accepted in the maintainer-requested review of PR #928, 2026-11-11.

## Context

The application previously resolved its AI session registry and agent override
files independently. Their historical paths can differ from the source used by
startup migration. PR #928 adds adoption when those paths are first resolved.

## Evidence

The initial adapter checked `current.exists()` and then called the ordinary
atomic replacement writer. A concurrently created target could be replaced;
a dangling destination symlink was also replaced because `exists()` follows it.
A partially populated agent directory suppressed all later copying. The new
partial-directory and dangling-symlink regressions both failed on that adapter.

Settings migration already has `write_if_absent`: it writes and syncs a temporary
file, then publishes without replacing a concurrent destination. Its existing
regressions exercise publication races and failure before publication.

## Decision

Expose `adopt_legacy_data_file` from the existing settings path authority. The
application adapter selects historical entries and enumerates regular files;
settings owns copy permissions, complete-file publication and cleanup. Existing
startup migration uses the same function. No new dependency is introduced.

Adoption fills missing files in an existing directory, preserves current files
and destination symlinks, and propagates enumeration/copy failures to the warning
at the application boundary. Paths remain cached for the process lifetime; an
incomplete copy is retried on the next process startup. Source files remain.

## Rejected alternatives

- Replacing atomic writes after an existence check: the check cannot serialize
  publication against other processes.
- A second non-replacing writer in the app: duplicates OS-specific publication
  and permission behavior already owned by settings.
- Skipping an existing directory: a failed first attempt permanently hides its
  remaining files.
- Increasing the platform-cfg budget: Windows credential target names and their
  compatibility reader instead share one platform adapter module in `sync.rs`.

## Consequences

This public settings helper is a narrow file-copy boundary; its caller supplies
an existing parent directory. Directory adoption is additive on first path use
per process. Removing an adopted rule while retaining its old source makes it
eligible for adoption at the next startup. Users can remove the old source when
retiring such an override.

## Validation

The two filesystem regressions failed before the fix and passed afterward.
All six directory tests and 107 settings tests passed in standalone Rust builds
of the production modules. Settings tests include interrupted publication,
concurrent destination creation and preservation of existing destination data.
Windows publication and the credential adapter require native CI validation.

## Supersedes

Refines the data-copy mechanism in
[legacy name inputs](../../pebrel_app/platform/2026-11-09-legacy-name-inputs.md).

## Revisit when

The historical application paths are retired, or adoption becomes an explicit
one-time user operation with its own completion record.
