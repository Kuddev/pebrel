# Windows distribution ownership

## Status

Implemented in the working tree; product release and Store certification pending.

## Context

Scoop manages versioned portable directories; Windows packages have a read-only
installation directory. The existing standalone updater downloads an Inno
installer and uses an adjacent startup lock. Applying that ownership model to
either channel wastes downloads and, for MSIX, attempts an invalid directory write.

## Evidence

- `nebula_app/src/platform/update_installation.rs` requires an Inno uninstaller
  before installation handoff; old portable copies already reject final handoff.
  The missing behavior was earlier channel-aware discovery/download and explanation,
  not a demonstrated successful overwrite of a Scoop install.
- `update_download/handoff.rs::installation_in_progress` used an installation-side
  lifetime lock before resident startup.
- `GetCurrentPackageFullName` reports real Windows package identity. A package
  identity does not prove that a package was acquired through Microsoft Store.
- `scripts/package-release.ps1` is the existing validated Windows payload authority.

## Decision

Detect package identity first and an installation-local `pebrel-distribution`
marker second. Scoop's pre-install hook writes the exact `scoop` marker. Missing
markers preserve the direct-distribution behavior; invalid/unreadable markers or
identity failures disable standalone updating. The marker grants no capabilities.

Cache the result once, initialized during the GUI startup ownership check. No
terminal render/input callback scans the filesystem or queries package metadata.
MSIX/Scoop do not start the standalone update-check worker, hydrate its cached
download, make a new download, schedule an installer or prepare its handoff.
They also skip the adjacent lock. Settings explain the owning channel and leave
the user's persisted update preferences untouched.

MSIX packaging wraps the existing portable packaging entry rather than duplicating
its payload list and freshness contract. Required Partner Center identity is an
input; generated MSIX remains unsigned. The initial package exposes the existing
native program and CLI alias, without declaring unimplemented Explorer integrations.
The console-subsystem alias requires `SupportsMultipleInstances=true` under SDK
semantic validation. This permits each CLI invocation to start its own process;
existing application/runtime logic still owns GUI and session handoff.

Scoop manifests are generated offline from published Release metadata; exact
architecture asset names remain owned by `scripts/stable_release.py`. The current
manifest targets existing v1.9.1 ZIPs; source updater improvements require a new
application release and are not attributed to those old binaries.

## Rejected alternatives

- A shared user setting for installation source: simultaneous installations would
  overwrite each other's update ownership.
- Inferring Scoop or Store from a directory-name substring: custom paths and
  copied directories make that unreliable.
- Packaging an Inno setup inside Scoop, or launching it for a Store package:
  that transfers ownership to a different installer.
- A new resident distribution service or scripting runtime: no lifecycle need.
- Treating every MSIX identity as proof of Store acquisition: sideloads exist.
- Duplicating the release payload and skipping MakeAppx validation: both allow
  divergence that the existing packaging contracts already prevent.

## Consequences

Channel detection is a small cold-path addition using an existing windows-sys
feature; no new crate, worker, VM or terminal hot-path callback is added. The
application package owner controls updates; plugin distribution remains separate.

Scoop's bucket publication and real installation acceptance are separate from
manifest generation. Store identity, signing for local acceptance, packaged OS
integration and certification are separate from generating a valid XML package.

Windows-only PowerShell scripts containing Chinese comments use UTF-8 with BOM
so Windows PowerShell 5.1 does not parse them using its legacy ANSI code page.

## Validation

Focused Rust tests cover marker parsing and installation-local ownership. A
test-only child-process case exercises real marker detection and the early update
boundaries with isolated process state. Existing updater tests remain in place.

Scoop generation was compared with the real v1.9.1 metadata, validated against the
upstream JSON schema, and checked with invalid draft/asset/digest inputs. Its
pre-install hook was executed only in a temporary directory.

Windows SDK 10.0.26100 MakeAppx accepted both x64 and ARM64 layout fixtures with
semantic validation enabled. Invalid identities and mismatched architecture were
rejected. These layout fixtures are not executable product packages or Store
acceptance results. Existing package architecture tests were also run.

## Supersedes

None.

## Revisit when

Real Store acceptance reveals a packaged data/OS integration requirement, another
distribution manager is supported, or the application introduces an explicitly
owned portable in-place updater.
