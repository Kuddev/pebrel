//! Exercise the real PSReadLine key maps without accessing the desktop clipboard.
#![cfg(windows)]

#[test]
fn native_clipboard_binding_preserves_user_keys_and_rechecks_the_active_map() {
    let script = include_str!("../src/tty/clipboard.ps1");
    let command = format!(
        r#"
$ErrorActionPreference = 'Stop'
Import-Module PSReadLine
$global:PebrelShellToken = 'fixture-owner'
Set-PSReadLineOption -EditMode Windows
{script}
if ((Get-PebrelClipboardBinding).Function -ne $global:PebrelClipboardHandlerName) {{ throw 'native handler not installed' }}
if ((Get-PebrelClipboardCapability) -ne 'fixture-owner') {{ throw 'missing owner capability' }}
{script}
if ((Get-PebrelClipboardBinding).Function -ne $global:PebrelClipboardHandlerName) {{ throw 'reload changed native binding' }}
Set-PSReadLineKeyHandler -Chord Ctrl+Shift+F11 -BriefDescription Paste -ScriptBlock {{ }}
if ((Get-PebrelClipboardCapability) -ne '') {{ throw 'user script named Paste was mistaken for native Paste' }}
{script}
if ((Get-PebrelClipboardBinding).Function -ne 'Paste') {{ throw 'same-name user handler was replaced' }}
if ((Get-PebrelClipboardCapability) -ne '') {{ throw 'reload claimed the user script named Paste' }}
Set-PSReadLineKeyHandler -Chord Ctrl+Shift+F11 -Function Paste
{script}
if ((Get-PebrelClipboardBinding).Function -ne 'Paste') {{ throw 'user-owned native binding was replaced' }}
if ((Get-PebrelClipboardCapability) -ne '') {{ throw 'user-owned native binding was claimed' }}
Set-PSReadLineKeyHandler -Chord Ctrl+Shift+F11 -Function SelfInsert
if ((Get-PebrelClipboardCapability) -ne '') {{ throw 'capability survived a changed binding' }}
{script}
if ((Get-PebrelClipboardBinding).Function -ne 'SelfInsert') {{ throw 'user binding was replaced' }}
Set-PSReadLineOption -EditMode Vi
{script}
if ((Get-PebrelClipboardBinding).Function -eq $global:PebrelClipboardHandlerName) {{ throw 'Vi keymap was changed despite being unsupported' }}
if ((Get-PebrelClipboardCapability) -ne '') {{ throw 'Vi map advertised a stable native binding' }}
Set-PSReadLineOption -EditMode Emacs
{script}
if ((Get-PebrelClipboardCapability) -ne 'fixture-owner') {{ throw 'Emacs native binding missing' }}
[Console]::Write('NATIVE_CLIPBOARD_BINDINGS_OK')
"#
    );
    let program =
        std::env::var_os("PEBREL_TEST_POWERSHELL").unwrap_or_else(|| "powershell.exe".into());
    let output = std::process::Command::new(program)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &command])
        .env("POWERSHELL_TELEMETRY_OPTOUT", "1")
        .output()
        .expect("start PowerShell with its bundled PSReadLine");
    assert!(
        output.status.success(),
        "PSReadLine binding checks failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("NATIVE_CLIPBOARD_BINDINGS_OK"));
}
