# System CIM objects are serialized only at the external JSON transport boundary.
$ErrorActionPreference = 'Stop'
$OutputEncoding = [Console]::OutputEncoding
$operatingSystem = Get-CimInstance Win32_OperatingSystem
$processors = @(Get-CimInstance Win32_Processor)
$cores = [int](($processors | Measure-Object NumberOfLogicalProcessors -Sum).Sum)
$bootTime = $operatingSystem.LastBootUpTime.ToUniversalTime()
$cpu = $null
$total = $null
$idle = $null
$timestamp = [DateTime]::UtcNow.ToFileTimeUtc() / 10000000.0
try {
    $processor = Get-CimInstance Win32_PerfRawData_PerfOS_Processor -Filter "Name='_Total'"
    if ($null -ne $processor -and $null -ne $processor.Timestamp_Sys100NS) {
        # The inverse idle counter and its reference clock retain the provider's normalization.
        $total = [uint64]$processor.Timestamp_Sys100NS
        $idle = [uint64]$processor.PercentProcessorTime
        $timestamp = [double]$processor.Timestamp_Sys100NS / 10000000.0
    }
} catch {
    # The independent memory/process snapshot remains usable without the performance provider.
    $total = $null
    $idle = $null
}
# Overview compares lightweight cumulative counters inside this invocation and emits five rows.
$properties = @('ProcessId','Name','CreationDate','KernelModeTime','UserModeTime','WorkingSetSize')
if ($includeProcesses) { $properties += @('ReadTransferCount','WriteTransferCount') }
$before = @{}
if (-not $includeProcesses) {
    foreach ($item in Get-CimInstance Win32_Process -Property $properties) {
        $before[[uint32]$item.ProcessId] = $item
    }
    $processSampleStart = [DateTime]::UtcNow
    Start-Sleep -Milliseconds 250
}
$processRows = @(Get-CimInstance Win32_Process -Property $properties)
$processSeconds = if ($includeProcesses) { 0 } else { ([DateTime]::UtcNow - $processSampleStart).TotalSeconds }
$processes = @(foreach ($process in $processRows) {
    if ($process.ProcessId -eq 0) { continue }
    $start = $null
    $ticks = $null
    $io = $null
    if ($null -ne $process.CreationDate) { $start = $process.CreationDate.ToUniversalTime().Ticks.ToString() }
    if ($null -ne $process.KernelModeTime -and $null -ne $process.UserModeTime) {
        $ticks = [uint64]$process.KernelModeTime + [uint64]$process.UserModeTime
    }
    if ($includeProcesses -and $null -ne $process.ReadTransferCount -and $null -ne $process.WriteTransferCount) {
        $io = [uint64]$process.ReadTransferCount + [uint64]$process.WriteTransferCount
    }
    $processCpu = $null
    if (-not $includeProcesses -and $processSeconds -gt 0 -and $cores -gt 0 -and $null -ne $ticks) {
        $old = $before[[uint32]$process.ProcessId]
        if ($null -ne $old -and $null -ne $process.CreationDate -and $old.CreationDate -eq $process.CreationDate -and $null -ne $old.KernelModeTime -and $null -ne $old.UserModeTime) {
            $oldTicks = [uint64]$old.KernelModeTime + [uint64]$old.UserModeTime
            if ($ticks -ge $oldTicks) { $processCpu = [Math]::Min(100.0, 100.0 * ($ticks - $oldTicks) / ($processSeconds * 10000000.0 * $cores)) }
        }
    }
    [pscustomobject]@{
        cpu = $processCpu
        pid = [uint32]$process.ProcessId; name = [string]$process.Name; start = $start
        ticks = $ticks; memory = [uint64]$process.WorkingSetSize; io_bytes = $io
    }
})
if (-not $includeProcesses) { $processes = @($processes | Sort-Object @{Expression='cpu';Descending=$true},pid | Select-Object -First 5) }
if ($processRows.Count -gt 16384) { throw 'Process collection limit exceeded' }
$diskError = $null
$disks = $null
try {
    $disks = @(Get-CimInstance Win32_LogicalDisk | Where-Object { $null -ne $_.Size -and $_.Size -gt 0 } | ForEach-Object {
        [pscustomobject]@{ mount = [string]$_.DeviceID; total = [uint64]$_.Size; available = [uint64]$_.FreeSpace; filesystem = [string]$_.FileSystem; source = [string]$_.DeviceID; root = $null; system = ($_.DeviceID -eq $operatingSystem.SystemDrive) }
    })
} catch { $diskError = $_.Exception.Message }
[pscustomobject]@{
    summary = [pscustomobject]@{
        name = [string]$operatingSystem.Caption; version = [string]$operatingSystem.Version
        uptime = [uint64](([DateTime]::UtcNow - $bootTime).TotalSeconds)
        free = [uint64]($operatingSystem.FreePhysicalMemory * 1024); cache = $null; cpu_cores = @(); cpu_user = $null; cpu_system = $null; network = $null
    }
    processes_complete = [bool]$includeProcesses
    boot = $bootTime.Ticks.ToString(); timestamp = $timestamp; total = $total; idle = $idle
    cpu = $cpu; ticks_per_second = 10000000; cores = $cores
    memory = [uint64](($operatingSystem.TotalVisibleMemorySize - $operatingSystem.FreePhysicalMemory) * 1024)
    total_memory = [uint64]($operatingSystem.TotalVisibleMemorySize * 1024)
    disks = $disks; disk_error = $diskError; processes = $processes
    nvidia_available = [bool](Get-Command nvidia-smi -CommandType Application -ErrorAction SilentlyContinue)
    docker_available = [bool](Get-Command docker -CommandType Application -ErrorAction SilentlyContinue)
    docker_host = $env:DOCKER_HOST; docker_context = $env:DOCKER_CONTEXT
} | ConvertTo-Json -Depth 5 -Compress
