$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$binary = Join-Path $repo 'target\debug\webext-lab-windows.exe'
if (-not (Test-Path -LiteralPath $binary)) { throw 'Build the Windows lab feature first.' }
$results = Join-Path $repo ('target\webext-host-rules\' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $results | Out-Null
$package = Join-Path $results 'package'
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'fixtures\host-rules') -Destination $package -Recurse
& $binary --prepare-windows $package '127.0.0.1'
if ($LASTEXITCODE -ne 0) { throw 'Host-rule manifest preparation failed.' }
$fetch = 'try { const response = await fetch("/dnr-probe"); return {blocked:false,status:response.status}; } catch(error) { return {blocked:true}; }'
$steps = @(
    @{ navigate = '$ORIGIN/baseline' }, @{ sleep = 700 },
    @{ label = 'uninstalled'; eval = $fetch },
    @{ load = $package }, @{ sleep = 700 },
    @{ label = 'allowed-host-rule'; eval = $fetch },
    @{ navigate = '$DENIED/denied' }, @{ sleep = 700 },
    @{ label = 'denied-host-rule'; eval = $fetch }
)
$scenario = Join-Path $results 'scenario.json'
[IO.File]::WriteAllText($scenario, (ConvertTo-Json -InputObject $steps -Depth 8), [Text.UTF8Encoding]::new($false))
$stdout = Join-Path $results 'results.jsonl'
$process = Start-Process -FilePath $binary -ArgumentList @("`"$scenario`"", "`"$(Join-Path $results 'data')`"") -WorkingDirectory $repo -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError (Join-Path $results 'stderr.log')
$processHandle = $process.Handle
if (-not $process.WaitForExit(60000)) { $process.Kill(); throw 'Host-rule qualifier timed out.' }
if ($process.ExitCode -ne 0) { throw "Host-rule qualifier exited with $($process.ExitCode)." }
$records = @(Get-Content -LiteralPath $stdout | ForEach-Object { $_ | ConvertFrom-Json })
foreach ($record in $records) {
    if ($record.kind -eq 'step' -and -not $record.value.ok) { throw 'Native lab step failed.' }
}
foreach ($label in @('uninstalled', 'allowed-host-rule', 'denied-host-rule')) {
    $matches = @($records | Where-Object { $_.kind -eq 'step' -and $_.value.label -eq $label })
    if ($matches.Count -ne 1) { throw "Missing result: $label" }
    $value = $matches[0].value.result.result.value
    $expected = $label -eq 'allowed-host-rule'
    if ($null -eq $value -or $null -eq $value.blocked -or $value.blocked -ne $expected) { throw "Unexpected native rule result: $label" }
    if (-not $expected -and $value.status -ne 200) { throw "Fixture request failed: $label" }
    Write-Output "$label passed"
}
Write-Output $results
