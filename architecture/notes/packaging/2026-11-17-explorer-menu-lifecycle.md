# Windows Explorer command installation lifecycle

## Status

Implemented; development-package lifecycle verified locally. Trusted signed
distribution and Windows ARM64 acceptance remain unverified.

## Context

The standalone Explorer command required a separate registration command after
copying files. An installed application needs installation, upgrade and removal
to own the per-user package as well as the files. The existing installer already
owns classic Explorer verbs; the new command supplies the Windows 11 menu.

## Evidence

Windows requires package identity for modern Explorer commands. Loose development
registration requires Developer Mode. Public sparse packages require a trusted
signature: see [Microsoft's deployment instructions](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps).
The local test host already enables development registration; that does not prove
unsigned deployment on a default Windows installation.

## Decision

The Inno builder builds/stages the standalone COM DLL and a short-lived native
deployment executable. The installer registers after copying files on Windows 11
and unregisters after uninstall confirmation, before removing files. Cancelling
uninstall leaves registration intact. Windows 10 retains its classic verbs.
The helper uses WinRT PackageManager directly, without requiring a scripting host,
linking application code into Explorer or changing application package identity.

`-ContextMenuPackage` accepts an externally signed sparse MSIX. The builder checks
its signature and exact manifest against the generated version, architecture and
extension, allowing only its publisher to differ. Windows also validates the
signature during installation. No signing keys or trust-policy mutations belong
in the installer. Without a supplied signed package, a local development build
uses loose registration and reports a failure if Windows refuses it. This is not
a claim that unsigned release builds work on ordinary end-user computers.

Package lookup is current-user and uses manifest name plus publisher. Ownership
uses the effective external directory, including signed packages whose package
files are stored separately in WindowsApps. A foreign installation cannot be
replaced during registration, and an old uninstaller leaves it alone. Failures
retain diagnostics; registration failure is visible and sets a nonzero setup
result while preserving the classic menu. Unregistration failure stops uninstall
before the registered files are removed.

## Rejected alternatives

- Enable Developer Mode or import certificates during installation: modifies
  machine trust rather than delivering a properly signed package.
- Require PowerShell 7 on end-user machines: unnecessary installer dependency.
- Use InstalledLocation as ownership: incorrect for signed sparse packages.
- Kill Explorer or all COM hosts during upgrade: affects unrelated extensions.
- Report success after a rejected signed package by retrying a development
  registration: hides a deployment error and changes its trust boundary.

## Consequences

The root Cargo workspace, terminal hot paths and GitHub workflows are unchanged.
The publishing maintainer must supply a trusted package for general distribution.
The helper package version follows the numeric application version. Payload
architecture and freshness checks include both helper binaries. Installed
diagnostic logs and stale package archives have explicit installer cleanup.

## Validation

Rust tests cover paths, manifest identity, URI escaping, COM errors and failed
launches. The Windows lifecycle test uses a unique package name and CLSID with
Unicode/space paths: register, repeat, version upgrade, foreign owner rejection,
foreign uninstall preservation, invalid signed archive rejection and idempotent
uninstall. It unregisters its own fixture package on exit. A minimal Inno fixture
executes the production include through install/reinstall, cancelled uninstall,
cleanup failure and successful uninstall. Both full product and isolated
acceptance installer definitions compile with syntax-only payloads. These checks
do not prove a complete current-version application release, signed deployment,
ARM64 execution or Explorer visual behavior on all Windows versions.

## Supersedes

The manual deployment boundary in
[the initial helper note](../tools/windows-context-menu/2026-11-08-local-explorer-extension.md).

## Revisit when

The release owner provisions signing, package identity changes, the Store build
adds its own command, or loaded-COM-server upgrade acceptance requires a different
file replacement policy.
