# Windows Explorer context menu

This standalone Windows COM DLL adapts the locally supplied
`E:\PebrelContextMenu` implementation. It adds the existing **Open in Pebrel**
command to the Windows 11 Explorer folder and folder-background menus.

Build alongside the Windows application (PowerShell 7):

```powershell
./scripts/build-windows-product.ps1 -Configuration release -TargetDirectory target -ContextMenu
```

Copy `target/release/context-menu` into `<installation>/context-menu`, alongside
`<installation>/pebrel.exe`, then register for the current Windows user:

```powershell
./scripts/install-windows-context-menu.ps1 -InstallationDirectory E:/Pebrel
```

The registration script uses the existing local sparse-package identity
`Pebrel.ContextMenu` / `CN=PebrelLocal`. It requires Windows to allow development
package registration; it does not change that policy or install certificates.
This local integration does not add signed MSIX distribution or change the
standard Inno installer/automatic updater. Reinstall/update the helper explicitly
if an installer replaces its files. Unregister before removing the installation:

```powershell
./scripts/install-windows-context-menu.ps1 -InstallationDirectory E:/Pebrel -Unregister
```

The DLL locates the executable relative to its own path, obtains folder paths
from Explorer and launches with separate arguments. No PowerShell/cmd command
text is built. The Explorer label comes from the existing installer translations
and follows the Windows UI language.

Validation:

```powershell
cargo test --locked --manifest-path tools/windows-context-menu/Cargo.toml --target-dir target/windows-context-menu
cargo run --locked --release --manifest-path tools/windows-context-menu/Cargo.toml --target-dir target/windows-context-menu --example probe
```

`probe --invoke <folder>` additionally invokes the actual registered command.
`probe --background <folder>` exercises the no-selection path with a real Windows
shell folder view supplied as the Explorer site.
A successful COM probe is not evidence that all Explorer visual/menu states have
been manually checked.
