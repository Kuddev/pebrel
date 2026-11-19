$ErrorActionPreference = 'Stop'
$compiler = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'
$root = New-Item -ItemType Directory -Path (Join-Path $env:RUNNER_TEMP 'relocated WSL')
$translationDir = New-Item -ItemType Directory -Force -Path target/installer-tools
$translation = Join-Path $translationDir.FullName 'ChineseSimplified.isl'
Invoke-WebRequest 'https://raw.githubusercontent.com/jrsoftware/issrc/c495623a97376d524f298b1b160e8fd612375c62/Files/Languages/ChineseSimplified.isl' -OutFile $translation
if ((Get-FileHash $translation).Hash -ne '6753BE2C5E2740D859900FD902824DB2EC568DA5C5B52486524C9762D778B0B0') { throw 'Translation digest mismatch' }

$payload = New-Item -ItemType Directory -Path (Join-Path $root 'payload')
foreach ($name in 'pebrel.exe','pebrel-cli.exe','pebrel-hook.exe','conpty.dll','OpenConsole.exe') {
    [IO.File]::WriteAllText((Join-Path $payload $name), 'Installer payload fixture')
}
function Build-Installer($version) {
    & $compiler '/Q' '/DAcceptanceFixture' "/DAppVersion=$version" "/DNumericVersion=$version.0" "/DBuildRoot=$payload" "/O$root" scripts/installer.iss | Out-Host
    if ($LASTEXITCODE) { throw "Compilation failed for $version" }
    return Join-Path $root "Pebrel-v$version-windows-x64-setup.exe"
}
function Install-At($setup, $dir, $label) {
    $log = Join-Path $root "$label.log"
    $rc = (Start-Process $setup -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/LANG=english',"/DIR=`"$dir`"","/LOG=`"$log`"") -Wait -PassThru).ExitCode
    Write-Host "$label exit=$rc"
    return $rc
}
$menus = @('HKCU:\Software\Classes\Directory\shell\PebrelWslMenu',
           'HKCU:\Software\Classes\Directory\Background\shell\PebrelWslMenu')
$distro = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Lxss\{8A72F0AF-ACC3-4522-967B-2490F9562E72}'
if ((Test-Path $menus[0]) -or (Test-Path $menus[1]) -or (Test-Path $distro)) { throw 'Fixture requires unused registry keys' }
New-Item -Path $distro -Force | Out-Null
Set-ItemProperty $distro DistributionName 'RelocationFixture'
function Assert-Menus($dir) {
    $exe = Join-Path $dir 'pebrel.exe'
    foreach ($i in 0..1) {
        $menu = $menus[$i]
        if ((Get-ItemPropertyValue $menu PebrelOwner) -ne $exe) { throw "Unexpected menu owner in $menu" }
        $arg = @('%1','%V')[$i]
        $expected = '"' + $exe + '" --gpui --shell "wsl:RelocationFixture" --working-directory "' + $arg + '"'
        $commands = @(Get-ChildItem "$menu\shell" | ForEach-Object { (Get-Item ($_.PSPath + '\command')).GetValue('') })
        if ($expected -notin $commands) { throw "Missing exact WSL command for $menu" }
    }
}
$old = Join-Path $root 'Previous path'
$new = Join-Path $root 'Changed path'
git restore --source=f939fddc -- scripts/installer-migration.iss
if ($LASTEXITCODE) { throw 'Parent checkout failed' }
$previous = Build-Installer '0.0.1'
try {
    ./scripts/tests/installer-migration.tests.ps1 -InnoCompiler $compiler
    throw 'The previous installer unexpectedly accepted its old menu'
} catch {
    if ($_.Exception.Message -notlike '*FAIL: Preserved an edited or foreign submenu*') { throw }
    Write-Host 'Reproduced the relocated-menu conflict on the previous source.'
}
if ((Install-At $previous $old 'original') -ne 0) { throw 'First installation failed' }
Assert-Menus $old
git restore --source=HEAD -- scripts/installer-migration.iss
if ($LASTEXITCODE) { throw 'Candidate checkout failed' }
./scripts/tests/installer-migration.tests.ps1 -InnoCompiler $compiler
$candidate = Build-Installer '0.0.2'
$edit = Join-Path $menus[1] 'CustomUserTree'
New-Item -Path $edit | Out-Null
Set-Item $edit 'keep'
if ((Install-At $candidate $new 'conflict') -eq 0) { throw 'Conflict should stop the upgrade' }
if ((Get-Content (Join-Path $root 'conflict.log') -Raw) -notlike '*WSL submenu*') { throw 'Missing WSL conflict in setup log' }
if ((Test-Path (Join-Path $new 'pebrel.exe')) -or (Get-Item $edit).GetValue('') -ne 'keep') { throw 'Conflicting install changed files or user data' }
Assert-Menus $old
Remove-Item $edit
if ((Install-At $candidate $new 'relocated') -ne 0) { throw 'Relocation failed' }
if (-not (Test-Path (Join-Path $new 'pebrel.exe'))) { throw 'Missing relocated executable' }
Assert-Menus $new
if ((Install-At $candidate $new 'same-directory') -ne 0) { throw 'Same-directory reinstall failed' }
Assert-Menus $new
Write-Host 'Passed real setup preflight, both old menus on conflict, relocated commands and in-place reinstall.'
