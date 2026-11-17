# Windows Explorer context menu

The Windows installer builds and installs this standalone COM extension. On
Windows 11 it automatically registers **Open in Pebrel** for folders and folder
backgrounds. Upgrade repeats registration; uninstall removes only the package
owned by that installation before deleting its files. Windows 10 keeps the
existing classic menu.

## Distribution prerequisite

Modern Explorer commands require package identity. **Public distribution needs
an MSIX signed by a certificate trusted on the destination computer.** Supply
that sparse package to the installer builder:

```powershell
./scripts/build-installer.ps1 -ContextMenuPackage C:/packages/Pebrel.ContextMenu.msix
```

The package must contain the generated `AppxManifest.xml` for this application
version and architecture. Its publisher may be changed to the signing certificate
subject; the other manifest content must match. Generate the manifest with
`scripts/build-windows-context-menu.ps1`, package that manifest alone with the
Windows SDK MakeAppx sparse-package procedure, then sign it. Signing credentials
are external inputs; this change does not configure release signing or workflows.

Without a signed package, local builds use development registration, which needs
Developer Mode already enabled. The installer never enables it or imports a
certificate. A registration failure is reported, logged to
`<installation>/context-menu/setup.log`, and gives a nonzero installer result;
the classic menu remains available. Do not distribute an unsigned development
build as if it supported normal end-user registration.

## Local build and validation

```powershell
./scripts/build-windows-product.ps1 -Configuration release -TargetDirectory target -ContextMenu
cargo test --locked --manifest-path tools/windows-context-menu/Cargo.toml --target-dir target/windows-context-menu
./scripts/tests/windows-context-menu.tests.ps1
```

The lifecycle test needs development registration and uses a unique package name
and CLSID. It does not modify the real installed menu. It covers repeat install,
upgrade, foreign ownership, rejected signed packages and uninstall.

`scripts/tests/installer-context-menu.tests.ps1 -InnoCompiler <ISCC.exe>` runs a
minimal installer using the production lifecycle include. It checks automatic
registration, reinstall, cancelled uninstall, failed cleanup and removal.

For a portable local copy, copy `target/release/context-menu` alongside
`<installation>/pebrel.exe`, then explicitly register or unregister:

```powershell
./scripts/install-windows-context-menu.ps1 -InstallationDirectory C:/apps/Pebrel
./scripts/install-windows-context-menu.ps1 -InstallationDirectory C:/apps/Pebrel -Unregister
```

The native helper uses Windows deployment APIs; installed users need no PowerShell
runtime. The DLL resolves the GUI executable relative to its own location and
passes folder paths as separate native arguments. Labels reuse installer English
and Chinese translations and follow the Windows UI language.

`cargo run --locked --release --manifest-path tools/windows-context-menu/Cargo.toml
--target-dir target/windows-context-menu --example probe` checks the registered
COM command. Its `--invoke <folder>` and `--background <folder>` options test
selected-folder and actual shell-folder-view paths. COM probes are not full
Explorer visual acceptance. The original helper was supplied locally by the
contributor and adapted into this standalone workspace.
