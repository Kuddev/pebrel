//! Check the actual in-box PSReadLine key maps, including the legacy API.
#![cfg(windows)]

#[test]
fn in_box_powershell_advertises_owned_pointer_bindings() {
    let editor = include_str!("../src/tty/completion.ps1");
    let pointer = include_str!("../src/tty/pointer.ps1");
    let command = format!(
        r#"
$ErrorActionPreference = 'Stop'
Import-Module PSReadLine
$global:PebrelShellToken = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('pwsh:fixture'))
foreach ($mode in 'Windows','Emacs') {{
    Set-PSReadLineOption -EditMode $mode
    {editor}
    {pointer}
    if (-not (Test-PebrelPointerBinding)) {{ throw "Native pointer unavailable in $mode" }}
    {editor}
    {pointer}
    if (-not (Test-PebrelPointerBinding)) {{ throw 'Reload revoked native pointer' }}
    Set-PSReadLineKeyHandler -Chord Ctrl+Shift+F10,1 -Function BeginningOfLine
    if (Test-PebrelPointerBinding) {{ throw 'Changed user key was claimed' }}
    {pointer}
    if (Test-PebrelPointerBinding) {{ throw 'Reload replaced user key' }}
    Set-PSReadLineKeyHandler -Chord Ctrl+Shift+F10,1 -BriefDescription $global:PebrelPointerHandlerName -ScriptBlock {{ }}
}}
Set-PSReadLineOption -EditMode Vi
if (Test-PebrelPointerBinding) {{ throw 'Unsupported Vi map was advertised' }}
[Console]::Write('NATIVE_POINTER_BINDINGS_OK')
"#
    );
    let program =
        std::env::var_os("PEBREL_TEST_POWERSHELL").unwrap_or_else(|| "powershell.exe".into());
    let output = std::process::Command::new(program)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &command])
        .env("POWERSHELL_TELEMETRY_OPTOUT", "1")
        .output()
        .expect("start in-box PowerShell with its native PSReadLine");
    assert!(
        output.status.success(),
        "PSReadLine pointer checks failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("NATIVE_POINTER_BINDINGS_OK"));
}
