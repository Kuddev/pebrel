# Cache shell discovery while preserving launch resolution

## Status

Implemented while reviewing PR #996.

## Context

Shell discovery reads the registry, PATH directories and WSL registrations.
Startup and restored panes repeated that work, including calls that only needed
an already selected shell. Frozen local launches and SSH/serial launches also
resolved the configured default even when they did not use it.

## Evidence

The author measured one discovery pass at about 17.1 ms on Windows 10 and about
five passes during a PowerShell startup. Those are measurements of that machine
and earlier prototype, not timings for this integrated revision.

The proposed ID-prefix shortcut also changed imported WSL profiles: their
opaque profile IDs do not identify the executable, so guest startup directories
were treated as host paths. Existing launch resolution already handles detected
shells, imported profiles and synthetic WSL IDs in one place.

## Decision

`shell_detect` owns a process-wide snapshot with serialized first initialization.
`detect_shells` returns a clone; explicit picker refresh replaces the snapshot.
An unresolved launch ID performs one fresh discovery pass before returning the
existing unknown-ID error. No timer or new dependency is introduced.

Resolve the configured default lazily only for local launches without a frozen
shell. Keep startup, resident handoff and cold-recovery directory decisions on
the existing resolved-program path. Imported WSL profiles therefore retain the
same guest/host policy as other WSL launches. The cache removes repeated OS
scans without substituting a different shell-selection rule.

## Rejected alternatives

- Classify from an ID prefix alone: opaque imported profiles can launch WSL too.
- Cache only in the legacy display adapter: GPUI startup would keep rescanning.
- Refresh on a timer: adds a lifetime and nondeterministic scan boundary.
- Treat a cache miss as the default shell: explicit invalid requests must fail.

## Consequences

Installed-shell lists refresh when a picker opens. An unresolved explicit launch
can refresh detection independently. Removed installations can remain listed
until refresh. Imported profiles continue to load through their existing owner.
Picker refresh remains synchronous, as discovery was before this change; the
snapshot bounds repeated work but does not make that cold path asynchronous.

## Validation

Production tests count discovery calls across repeated lookup and refresh, keep
unknown IDs unresolved, and require one rescan on a launch-resolution miss.
Existing startup-directory, imported-profile and WSL tests remain in the suite.
Integration retains the original program-based directory call sites; native CI
validates this final head before merge.

## Supersedes

None.

## Revisit when

Shell installation notifications provide a reliable invalidation source, or
profile and installed-shell discovery move behind a shared asynchronous owner.
