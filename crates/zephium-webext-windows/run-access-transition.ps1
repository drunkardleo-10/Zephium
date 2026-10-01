$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$binary = Join-Path $repo 'target\debug\webext-lab-windows.exe'
$results = Join-Path $repo ('target\webext-access-transition\' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $results -Force | Out-Null
$all = Join-Path $results 'all'
$specific = Join-Path $results 'specific'
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'fixtures\storage') -Destination $all -Recurse
Copy-Item -LiteralPath $all -Destination $specific -Recurse
& $binary --prepare-windows $specific '127.0.0.1'
if ($LASTEXITCODE -ne 0) { throw 'Preparation failed.' }
$steps = @(
    @{load=$all}, @{view='popup'}, @{in='popup';popup=$true}, @{sleep=700},
    @{in='popup';eval="await chrome.storage.local.set({qaValue:'survives'}); return chrome.runtime.getManifest().host_permissions";label='all-manifest'},
    @{navigate='$DENIED/before'}, @{sleep=700},
    @{eval="return document.documentElement.dataset.zephiumStorageQualifier === 'injected'";label='before-denied-injects'},
    @{enabled=$false}, @{in='popup';navigate='about:blank'},
    @{load=$specific}, @{enabled=$true}, @{in='popup';popup=$true}, @{sleep=700},
    @{in='popup';eval="return {data:await chrome.storage.local.get('qaValue'),hosts:chrome.runtime.getManifest().host_permissions}";label='specific-data-and-manifest'},
    @{navigate='$DENIED/after'}, @{sleep=700},
    @{eval="return document.documentElement.dataset.zephiumStorageQualifier === 'injected'";label='after-denied-injects'},
    @{navigate='$ORIGIN/after'}, @{sleep=700},
    @{eval="return document.documentElement.dataset.zephiumStorageQualifier === 'injected'";label='after-allowed-injects'}
)
$scenario = Join-Path $results 'scenario.json'
[IO.File]::WriteAllText($scenario, (ConvertTo-Json -InputObject $steps -Depth 8), [Text.UTF8Encoding]::new($false))
$stdout = Join-Path $results 'results.jsonl'
$process = Start-Process -FilePath $binary -ArgumentList @("`"$scenario`"", "`"$(Join-Path $results 'data')`"") -WorkingDirectory $repo -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError (Join-Path $results 'stderr.log')
$processHandle = $process.Handle
if (-not $process.WaitForExit(60000)) { $process.Kill(); throw 'Access transition timed out.' }
if ($process.ExitCode -ne 0) { throw 'Access transition lab failed.' }
$records = @(Get-Content $stdout | ForEach-Object { $_ | ConvertFrom-Json })
if (@($records | Where-Object { $_.kind -eq 'step' -and (-not $_.value.ok -or $_.value.result.PSObject.Properties['exceptionDetails']) }).Count) { throw 'Native step failed.' }
function Result-For([string]$Label) {
    $matches = @($records | Where-Object { $_.value.label -eq $Label })
    if ($matches.Count -ne 1) { throw "Missing result: $Label" }
    $matches[0].value.result.result.value
}
$specificResult = Result-For 'specific-data-and-manifest'
if ($specificResult.data.qaValue -ne 'survives' -or @($specificResult.hosts).Count -ne 1 -or $specificResult.hosts[0] -ne '*://127.0.0.1/*') { throw 'Replacement lost data or kept broad permissions.' }
if ((Result-For 'before-denied-injects') -ne $true -or (Result-For 'after-denied-injects') -ne $false -or (Result-For 'after-allowed-injects') -ne $true) { throw 'Content-script narrowing did not take effect.' }
Write-Output "Storage and native manifest replacement passed: $results"
