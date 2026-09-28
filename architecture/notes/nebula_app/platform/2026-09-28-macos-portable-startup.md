# macOS portable startup storage

## Status

Implemented; pending review and native dialog acceptance.

## Context

Moving Pebrel.app alone leaves settings on the host. Storage must be selected before migration, logging, path caches or workers start.

## Evidence

The existing shared settings directory controls configuration, sessions and history. A macOS application bundle supplies a stable location from which to derive a sibling data directory after a move. The startup adapter and its focused tests document the current behavior.

## Decision

A macOS bundle outside `/Applications`, `/System/Applications` and `~/Applications` offers portable, normal or quit. Explicit configuration and unbundled executables retain their existing behavior. Portable mode stores data in sibling `Pebrel Data`, remembers acceptance with `.pebrel-portable`, and sets both configuration directory aliases plus `TMPDIR` before workers start. CLI helpers use an existing portable store without a prompt. Portable startup skips host legacy migration. A linked or unwritable data root stops portable startup. An eligible translocated bundle without an explicit override stops before any launch choice rather than falling back to host settings.

## Rejected alternatives

- A new container format would change every persistence consumer.
- Writing inside the signed bundle would make data depend on application replacement.
- Persisting an absolute data path would break when the app and data move together.

## Consequences

Users move Pebrel.app and `Pebrel Data` together after quitting. Existing preferences can be imported using Backup. System credentials, SSH keys, external tools, project files and OS caches remain outside the portable directory; live processes and absolute paths in user configuration do not migrate. Windows and Linux startup remain unchanged.

## Validation

Focused tests cover bundle location, relaunch, folder moves, unavailable or redirected storage, and localized choices. Native compilation and actual dialog interaction require separate evidence.

## Supersedes

None.

## Revisit when

Another platform needs portable storage or automatic profile migration has an explicit compatibility contract and native verification.
