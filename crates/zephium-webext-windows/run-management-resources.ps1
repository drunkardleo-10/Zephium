param([ValidateRange(60, 600)][int]$IdleSeconds = 120)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$binary = Join-Path $repo 'target\debug\webext-lab-windows.exe'
$results = Join-Path $repo ('target\webext-management-resources\' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $results -Force | Out-Null
$suite = Get-Content (Join-Path $repo 'crates\zephium-webext-macos\suite.json') -Raw | ConvertFrom-Json
$names = @('Bitwarden', 'Dark Reader', 'Grammarly', 'Vimium', 'SponsorBlock')
$packages = @($names | ForEach-Object {
    $entry = $suite | Where-Object name -eq $_
    $path = Join-Path $repo "target\webext-suite\$($entry.id).crx"
    if (-not (Test-Path -LiteralPath $path)) { throw "Missing package: $path" }
    $path
})
foreach ($count in @(1, 3, 5)) {
    foreach ($mode in @('persistent', 'shared')) {
        $label = "$count-$mode"
        $steps = [Collections.Generic.List[object]]::new()
        $steps.Add(@{navigate='$ORIGIN/first'})
        if ($mode -eq 'shared') { $steps.Add(@{view='manager'}) }
        for ($index = 0; $index -lt $count; $index++) {
            $steps.Add(@{load=$packages[$index]; prepare_windows=$true})
            $manager = if ($mode -eq 'shared') { 'manager' } else { "manager$index" }
            if ($mode -eq 'persistent') { $steps.Add(@{view=$manager}) }
            $steps.Add(@{in=$manager; navigate='chrome-extension://$ID/zephium-windows-host/host.html'})
            $steps.Add(@{sleep=2000})
            if ($mode -eq 'shared') { $steps.Add(@{in=$manager; navigate='about:blank'}) }
        }
        $steps.Add(@{list=$true; label='installed inventory'})
        $steps.Add(@{cdp='Target.getTargets'; label='targets before idle'})
        $steps.Add(@{sleep=($IdleSeconds * 1000); label='uninstrumented idle'})
        $steps.Add(@{cdp='Target.getTargets'; label='targets after idle'})
        $scenario = Join-Path $results "$label.json"
        [IO.File]::WriteAllText($scenario, (ConvertTo-Json -InputObject @($steps.ToArray()) -Depth 12), [Text.UTF8Encoding]::new($false))
        $stdout = Join-Path $results "$label.jsonl"
        $process = Start-Process -FilePath $binary -ArgumentList @("`"$scenario`"", "`"$(Join-Path $results "$label.data")`"") -WorkingDirectory $repo -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError (Join-Path $results "$label.stderr.log")
        $processHandle = $process.Handle
        $clock = [Diagnostics.Stopwatch]::StartNew()
        while (-not $process.HasExited) {
            if ($clock.Elapsed.TotalSeconds -gt $IdleSeconds + 150) { throw "Lab exceeded its deadline: $label (PID $($process.Id))" }
            $inventory = @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId)
            $ids = [Collections.Generic.HashSet[int]]::new()
            [void]$ids.Add($process.Id)
            do {
                $added = $false
                foreach ($entry in $inventory) {
                    if ($ids.Contains([int]$entry.ParentProcessId) -and $ids.Add([int]$entry.ProcessId)) { $added = $true }
                }
            } while ($added)
            $memory = 0L; $cpu = 0.0; $processCount = 0
            foreach ($processId in $ids) {
                $child = Get-Process -Id $processId -ErrorAction SilentlyContinue
                if ($child) { $memory += $child.PrivateMemorySize64; $cpu += $child.CPU; $processCount++ }
            }
            [pscustomobject]@{seconds=$clock.Elapsed.TotalSeconds; privateMiB=($memory / 1MB); processes=$processCount; cpuSeconds=$cpu} | Export-Csv (Join-Path $results "$label.csv") -NoTypeInformation -Append
            Start-Sleep -Seconds 5
        }
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw "Lab failed: $label" }
        $records = @(Get-Content $stdout | ForEach-Object { $_ | ConvertFrom-Json })
        if (@($records | Where-Object { $_.kind -eq 'step' -and -not $_.value.ok }).Count) { throw "A native step failed: $label" }
        Write-Output "Completed $label"
    }
}
Write-Output $results
