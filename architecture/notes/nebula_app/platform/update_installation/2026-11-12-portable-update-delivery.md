# Windows portable copies check releases for a ZIP

## Status

Proposed for [#533](https://github.com/Kuddev/pebrel/issues/533).

## Context

Windows ZIP copies could download and verify a setup executable before the
installation handoff refused it. An earlier update check needs to distinguish
the release channel from the owner of an installation.

## Evidence

The published 2.5.3 x64 and ARM64 ZIPs contain the respective native executable
without an adjacent `unins000.exe`. Both architectures also have a separate
setup asset. Before this change, the update check selected that setup for a
direct distribution regardless of its installation layout.

## Decision

After checking Windows package identity and the distribution marker, classify
unpacked copies using the same local uninstaller check as the setup handoff.
Keep release checks for portable copies, and expose the matching ZIP as a
browser download. A direct link must name an exact asset under the official
release; the rate-limit fallback uses the official checksum manifest. If that
link cannot be established, offer the release page.

Portable copies cannot start or hydrate a setup download, consume a scheduled
installation, or acquire an installation lock. This leaves installer state
available to the installed copy that owns it. Portable updates never enter the
installer downloader or transaction state machine.

## Rejected alternatives

- Treating the ZIP as an installer asset would reuse the wrong validation and
  replacement contract. An automatic portable replacement needs its own exit,
  file ownership and rollback design.
- Deleting old cached installers or scheduled state could affect an installed
  copy using the same configuration directory.
- Matching a directory name would confuse user-selected setup locations with
  extracted copies.

## Consequences

Existing update checks remain available for portable users. They get the ZIP for
the running executable's architecture and can extract it after exiting Pebrel.
Installed copies still use the verified setup flow. External distribution
channels retain their original ownership.

## Validation

The 2.5.3 public asset hashes, both ZIPs' CRCs, executable architectures, and
portable directory layouts were checked. Tests cover both architectures,
missing ZIPs, untrusted URLs, checksum fallback, and the Windows process-level
installer entry points.

## Supersedes

None. This extends the direct-distribution behavior in the [Windows distribution
ownership note](../../../packaging/2026-09-28-windows-distribution-ownership.md).

## Revisit when

An updater is designed to replace a portable tree and recover safely from a
failed replacement.
