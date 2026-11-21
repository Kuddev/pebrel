param([string] $Taskbar, [string] $Overflow)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$tree = [Windows.Automation.TreeScope]::Descendants
$condition = [Windows.Automation.Condition]::TrueCondition
$seen = [System.Collections.Generic.List[string]]::new()
$opened = $false
foreach ($value in @($Taskbar, $Overflow)) {
    if ($value -eq '0') { continue }
    $root = [Windows.Automation.AutomationElement]::FromHandle([IntPtr]::new([long]$value))
    foreach ($item in $root.FindAll($tree, $condition)) {
        $name = $item.Current.Name
        if (-not $name) { continue }
        $seen.Add($name)
        $bounds = $item.Current.BoundingRectangle
        if ($name -like 'Pebrel*' -and -not $bounds.IsEmpty -and -not $item.Current.IsOffscreen) {
            [pscustomobject]@{ X = $bounds.X; Y = $bounds.Y; Width = $bounds.Width; Height = $bounds.Height } | ConvertTo-Json -Compress
            exit 0
        }
        if ($Overflow -eq '0' -and -not $opened -and $name -match 'Show hidden icons|notification chevron') {
            $pattern = $null
            if ($item.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
                $pattern.Invoke()
                $opened = $true
            }
        }
    }
}
[pscustomobject]@{ Observed = ($seen -join ', ') } | ConvertTo-Json -Compress
