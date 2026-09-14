[CmdletBinding()]
param([string] $TargetDirectory)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$installerPath = Join-Path $repo 'scripts\installer.iss'
$migrationPath = Join-Path $repo 'scripts\installer-migration.iss'
$builderPath = Join-Path $repo 'scripts\build-installer.ps1'

$installer = Get-Content -LiteralPath $installerPath -Raw -Encoding UTF8
$migration = Get-Content -LiteralPath $migrationPath -Raw -Encoding UTF8
$requiredPatterns = [ordered]@{
    'migration-aware installation directory' = 'DefaultDirName=\{code:DefaultInstallDir\}'
    'explicit previous-directory migration' = 'UsePreviousAppDir=no'
    'Pebrel start-menu group' = 'UsePreviousGroup=no'
    'non-admin installation' = 'PrivilegesRequired=lowest'
    'Windows 10 1809 floor' = 'MinVersion=10\.0\.17763'
    'application closing' = 'CloseApplications=yes'
    'no application restart during uninstall' = 'RestartApplications=no'
    'desktop shortcut task' = 'Tasks: desktopicon'
    'login startup task' = '\{userstartup\}\\Pebrel'
    'hook cleanup command' = 'Parameters: "setup-ai --remove"'
    'gpui start-menu shortcut' = 'Parameters: "--gpui"'
    'idempotent cleanup entry' = 'RunOnceId: "RemovePebrelAiHooks"'
    'hook helper payload' = 'pebrel-hook\.exe'
    'ConPTY payload' = 'conpty\.dll'
    'ConPTY host payload' = 'OpenConsole\.exe'
    'font payload' = 'MapleMonoNormal-NF-CN-Regular\.ttf'
    'optional font installation task' = 'Tasks: installfont'
    'pinned Chinese language file' = 'target\\installer-tools\\ChineseSimplified\.isl'
    'localized context menu label' = 'english\.OpenInPebrel=Open in Pebrel'
    'Pebrel display name' = 'AppName=Pebrel'
    'compatible installer identity' = 'AppId=\{\{61022144-7D0A-4E54-94F2-C329A8F58656\}'
    'Pebrel default asset name' = '#define PackageBrand "Pebrel"'
    'explicit package brand' = 'OutputBaseFilename=\{#PackageBrand\}-v\{#AppVersion\}-windows-x64-setup'
    'localized Chinese context menu label' = 'chinesesimplified\.OpenInPebrel=\S.+'
    'localized WSL context menu label' = 'english\.OpenInPebrelWsl=Open in Pebrel'
    'localized Chinese WSL context menu label' = 'chinesesimplified\.OpenInPebrelWsl=\S.+'
    'WSL context menu uninstall cleanup' = 'RemoveOwnedWslContextMenus;'
    'directory background context menu' = 'Software\\Classes\\Directory\\Background\\shell\\Pebrel'
    'selected directory context menu' = 'Software\\Classes\\Directory\\shell\\Pebrel'
    'context menu executable icon' = 'ValueName: "Icon"; ValueData: "\{app\}\\pebrel\.exe,0"'
    'background working-directory command' = '--gpui --working-directory ""%V""'
    'selected directory working-directory command' = '--gpui --working-directory ""%1""'
    'PATH task' = 'Name: "addtopath"; Description: "\{cm:AddToPath\}"'
    'PATH registry entry' = 'Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"'
    'PATH ownership marker' = 'ValueName: "InstallerAddedToPath"'
    'Win+R App Paths registration' = 'App Paths\\pebrel\.exe'
    'notification identity on shortcuts' = 'AppUserModelID: "com\.pebrel\.terminal"'
    'environment change notification' = 'ChangesEnvironment=yes'
    'PATH uninstall cleanup' = 'CurUninstallStepChanged\(CurUninstallStep: TUninstallStep\)'
    'runtime control API documentation' = 'Source: "\{#RepoRoot\}\\docs\\runtime-control-api\.md"; DestDir: "\{app\}\\docs";'
    'runtime API schema' = 'Source: "\{#RepoRoot\}\\docs\\runtime-api-v1\.schema\.json"; DestDir: "\{app\}\\docs";'
    'Pebrel Runtime skill instructions' = 'Source: "\{#RepoRoot\}\\docs\\skills\\pebrel-runtime\\SKILL\.md"; DestDir: "\{app\}\\skills\\pebrel-runtime";'
    'Pebrel Runtime skill metadata' = 'Source: "\{#RepoRoot\}\\docs\\skills\\pebrel-runtime\\agents\\openai\.yaml"; DestDir: "\{app\}\\skills\\pebrel-runtime\\agents";'
}

foreach ($entry in $requiredPatterns.GetEnumerator()) {
    if ($installer -notmatch $entry.Value) {
        throw "Installer is missing $($entry.Key): $($entry.Value)"
    }
}

$uninstallRun = $installer.IndexOf('[UninstallRun]', [System.StringComparison]::Ordinal)
$cleanup = $installer.IndexOf('setup-ai --remove', [System.StringComparison]::Ordinal)
if ($uninstallRun -lt 0 -or $cleanup -lt $uninstallRun) {
    throw 'Hook cleanup must be an [UninstallRun] action so it executes before installed files are deleted.'
}

$contextMenuRoots = @(
    'Software\Classes\Directory\Background\shell\Pebrel'
    'Software\Classes\Directory\shell\Pebrel'
)
foreach ($root in $contextMenuRoots) {
    $escapedRoot = [regex]::Escape($root)
    if ($installer -notmatch "Subkey: `"$escapedRoot`";.*Flags: uninsdeletekey") {
        throw "Context-menu key must be removed during uninstall: $root"
    }
}

$migrationPatterns = [ordered]@{
    'per-user Pebrel default' = '\{localappdata\}\\Programs\\Pebrel'
    'registered previous installation' = 'Inno Setup: App Path'
    'known previous directory rename' = "ExtractFileName\(Result\), 'Nebula Terminal'"
    'running legacy executable detection' = 'IsExecutableRunning\(Executable\)'
    'retryable migration record' = 'PendingLegacyInstallDir'
    'visible migration failures' = "CustomMessage\('MigrationFailed'\)"
    'nonzero migration failure exit' = 'GetCustomSetupExitCode'
    'precise legacy payload cleanup' = 'RemoveLegacyPayload'
    'linked path protection' = 'Attributes and \$400'
}
foreach ($entry in $migrationPatterns.GetEnumerator()) {
    if ($migration -notmatch $entry.Value) {
        throw "Installer migration is missing $($entry.Key): $($entry.Value)"
    }
}
if ($migration -match 'DelTree\(|TerminateProcess\(|taskkill') {
    throw 'Migration must not recursively delete user data or forcefully terminate applications.'
}

# 按 WSL 发行版注册的右键项（installer-migration.iss 里的 [Code]）：
$wslPatterns = [ordered]@{
    'WSL distribution registry' = 'Software\\Microsoft\\Windows\\CurrentVersion\\Lxss'
    'plumbing distros are skipped' = "Pos\('docker-desktop'"
    'distinct verb namespace' = "'PebrelWsl' \+ IntToStr\(Index\)"
    'registration runs at post-install' = 'RegisterWslContextMenus;'
    'owned keys are reclaimed by prefix and command' = "Pos\('PebrelWsl', Names\[NameIndex\]\) = 1"
    'uninstall sweeps both roots' = "Directory\\Background\\shell'"
}
foreach ($entry in $wslPatterns.GetEnumerator()) {
    if ($migration -notmatch $entry.Value) {
        throw "Installer migration is missing $($entry.Key): $($entry.Value)"
    }
}

# 命令串里 `--shell` 必须排在 `--working-directory` 之前：盘根（`D:\`）时后者的
# 收尾反斜杠会吃掉收尾引号并把后面整段并进同一个参数（issue #36 的另一面），顺序
# 写反会静默开出一个既没有 cwd、也没用上指定发行版的标签。
$commandTemplates = @(
    $migration -split "`n" | Where-Object { $_ -match '^\s*Command :=' -and $_ -match '--shell' }
)
if ($commandTemplates.Count -ne 1) {
    throw "Expected exactly one WSL context-menu command template, found $($commandTemplates.Count)."
}
$shellAt = $commandTemplates[0].IndexOf('--shell')
$directoryAt = $commandTemplates[0].IndexOf('--working-directory')
if ($shellAt -lt 0 -or $directoryAt -lt 0 -or $shellAt -gt $directoryAt) {
    throw "The WSL context-menu command must pass --shell before --working-directory: $($commandTemplates[0].Trim())"
}

$validationArguments = @{ SkipBuild = $true; AllowStale = $true; ValidateOnly = $true }
if (-not [string]::IsNullOrWhiteSpace($TargetDirectory)) {
    $validationArguments.TargetDirectory = $TargetDirectory
}
& $builderPath @validationArguments

$builder = Get-Content -LiteralPath $builderPath -Raw -Encoding UTF8
if ($builder -notmatch 'Stale binary') {
    throw 'build-installer.ps1 must refuse stale binaries (freshness guard missing).'
}
if ($builder -notmatch 'c495623a97376d524f298b1b160e8fd612375c62' -or
    $builder -notmatch '6753BE2C5E2740D859900FD902824DB2EC568DA5C5B52486524C9762D778B0B0') {
    throw 'The Chinese installer translation must use a pinned source commit and SHA-256.'
}

Write-Output "installer.tests.ps1: PASS ($($requiredPatterns.Count) invariants)"
