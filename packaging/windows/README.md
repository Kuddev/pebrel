# Microsoft Store / MSIX packaging

The existing native Rust/GPUI product is packaged as a full-trust Win32 app.
There is no UWP/WinUI rewrite and no additional application runtime.

## 1. Obtain the product identity

Sign in to [Partner Center](https://partner.microsoft.com/dashboard), complete
the Windows developer enrollment if needed, create/reserve the product, and
copy these values from its product identity page:

- `Package/Identity/Name`
- `Package/Identity/Publisher`
- `PublisherDisplayName`

These are required build inputs. The script does not guess a publisher, create
an account, submit a package, sign an agreement, or reserve a product name.
Enrollment requirements depend on account type and region.

## 2. Build a package

Use the project's pinned Rust toolchain, ordinary product build prerequisites,
and the Windows SDK with `MakeAppx.exe`. From a Windows checkout:

```powershell
.\scripts\package-msix.ps1 `
  -Version 'VERSION' `
  -IdentityName 'IDENTITY_NAME' `
  -Publisher 'CN=PUBLISHER_ID_FROM_PARTNER_CENTER' `
  -PublisherDisplayName 'PUBLISHER_DISPLAY_NAME' `
  -Architecture x64 `
  -TargetDirectory 'target\store-build' `
  -OutputDirectory 'dist\store'
```

Replace every identity placeholder and `VERSION` before execution. `Version`
must match the current stable product version. MSIX uses `major.minor.patch.0`.
Build ARM64 on the existing native ARM64 build environment with `-Architecture
arm64`; the argument validates payload architecture and does not install a
cross-compilation toolchain. A mismatched application or console helper fails.

`-MakeAppxPath` can select an SDK tool explicitly. `-Configuration debug` is for
local packaging investigation, not Store submission. Release is the default.

The wrapper invokes `package-release.ps1` without skip/staleness overrides,
thereby reusing its fresh product build, version check, complete resource layout,
and executable-architecture validation. It then:

1. Extracts its own validated portable package into an isolated staging folder.
2. Generates the XML identity and three logo sizes from the existing product icon.
3. Declares `Windows.FullTrustApplication`, `runFullTrust`, and the `pebrel.exe`
   application execution alias with its console subsystem for CLI use.
4. Runs `MakeAppx pack` with validation enabled; `/nv` is not used.
5. Produces `Pebrel-vVERSION-windows-ARCH.msix` and reports its SHA256.

It does not overwrite an existing MSIX output. Staging cleanup is limited to the
unique directory created by that invocation.

## 3. Signing and submission

The generated package is **unsigned**. Microsoft's current requirements state
that Store MSIX/Appx submissions do not require a CA-trusted signing certificate;
the Store signs/re-signs the package in its publishing process. This differs from
EXE/MSI submission, which requires developer-side Authenticode signing.

For local sideload acceptance, use a separately approved test identity and test
certificate matching its Publisher, sign with the Windows SDK, then install in
an isolated Windows test environment. Certificate trust changes and package
installation are not performed by the packaging script. Keep private keys out
of the repository. Use the real Partner Center identity for the Store candidate.

Complete the Store listing, screenshots, privacy-policy link, supported languages
and capability/certification notes. Run Windows App Certification Kit and submit
the candidate through Partner Center. A generated package or successful SDK
validation is not a Store certification result.

## Integration acceptance still required

- Start-menu launch and `pebrel.exe` alias, CLI exit codes and working directory.
- ConPTY, PowerShell/cmd, WSL, SSH and lookup of packaged console/helper files.
- Runtime discovery and communication with commands started outside the package.
- Existing configuration import, packaged AppData behavior, plugin/config paths
  and uninstall retention. Do not assume a packaged path equals the unpackaged path.
- A real old-package to new-package upgrade; UI, input and active sessions.
- Explorer context menus, startup registration, notifications and optional font
  installation. The initial manifest does not reproduce Inno registry entries
  or claim these integrations have been accepted under MSIX.

The application recognizes a real Windows package identity and leaves updates
to that package's source. The UI says MSIX rather than asserting that a sideloaded
package came from the Store. It skips the standalone updater's adjacent startup
lock because the MSIX installation directory is read-only.

MSIX artifacts are currently a separate explicit packaging entry, not additional
required assets in the stable GitHub Release transaction. Enable Store publishing
automation after the real identity and integration acceptance are complete; do
not append an unvalidated MSIX to an existing stable release's exact asset set.

References:

- [Build MSIX from native app binaries](https://learn.microsoft.com/en-us/windows/msix/desktop/source-code-overview)
- [MakeAppx](https://learn.microsoft.com/en-us/windows/msix/package/create-app-package-with-makeappx-tool)
- [Store MSIX requirements](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements)
- [Packaged desktop behavior](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-prepare)
