# Unix package helper and desktop entry completeness

## Status

Implemented; packaging validation does not constitute a release.

## Context

Agent hooks need a helper beside the executable. Building only the main binary
produced Unix packages whose visible Agent integration could not report events.

## Evidence

- `scripts/package-linux.sh` installs the executable helper into the common package root.
- `scripts/package-macos.sh` includes and signs the helper inside the application bundle.
- Release and Preview workflows build both binaries from the same checkout.

## Decision

Require a helper of the matching architecture during packaging, with its executable
permission retained. On macOS sign it before signing the enclosing bundle. Reuse
the same Linux package root for AppImage, portable archive and Debian packaging.

Linux desktop entries advertise directory handling and pass a directory through
the existing terminal startup path. The macOS bundle advertises `public.folder`
as an alternate handler; application open-URL events use the shared window dispatcher.
These entries do not change the user's default folder handler.

Login startup uses a per-user LaunchAgent or XDG autostart entry, quoting according
to the native file format rather than a shell. Platforms without an installation
transaction skip updater sidecar creation, so a read-only AppImage mount or `/usr/bin`
does not prevent ordinary startup.

## Rejected alternatives

- Letting packaging omit the helper would defer a deterministic error to runtime.
- Running the Windows installation handoff on Linux would not install a Unix package.
- Treating a directory handler as the system's default file manager would exceed
  the requested integration.

## Consequences

Standalone developer builds still need the helper built alongside the application
to install Agent hooks. Linux in-app binary replacement remains a separate updater
capability; skipping its lock is not an implementation of package installation.

## Validation

Existing release/Preview metadata and macOS package tests check the package contract.
Shell syntax and desktop-entry validation check the edited inputs. No version,
published asset, or release tag is changed by this work.

## Supersedes

None.

## Revisit when

The Unix runtime layout changes or Linux gains a verified package-ownership-aware
installation transaction.
