use super::{NEBULA_PROMPT_PS1, PS_PRELUDE, run_powershell_case};

#[test]
fn builtin_ls_aliases_can_be_opted_out() {
    let gate = NEBULA_PROMPT_PS1
        .find("if (Get-NebulaBoolSetting 'builtin_ls' $true) {")
        .expect("builtin_ls gate");
    let alias = NEBULA_PROMPT_PS1.find("Set-Alias -Name ls ").expect("ls alias");
    assert!(gate < alias, "the ls/ll/dir aliases must sit behind the builtin_ls gate");
}

#[test]
fn powershell_ls_retains_nonfilesystem_provider_objects() {
    let checks = r#"
$ErrorActionPreference = 'Stop'
$env:PEBREL_LIST_FIXTURE = 'provider-value'
if ((Get-Alias ls).Definition -ne 'Nebula-List') { throw 'ls integration not active' }
$entry = ls Env: | Where-Object Name -eq 'PEBREL_LIST_FIXTURE'
if ($entry.Value -ne 'provider-value') { throw 'Environment object lost' }
Push-Location Alias:
try {
if ((ls ls) -isnot [System.Management.Automation.AliasInfo]) { throw 'Alias object lost' }
} finally { Pop-Location }
if ((ls $PSCommandPath) -notmatch 'rw-r--r--') { throw 'File listing lost its formatting' }
"#;
    for executable in ["powershell", "pwsh"] {
        run_powershell_case(executable, PS_PRELUDE, checks);
    }
}
