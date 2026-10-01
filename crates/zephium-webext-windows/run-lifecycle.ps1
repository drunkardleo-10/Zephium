$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$run = Join-Path $repo ('target\webext-lifecycle\' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $run -Force | Out-Null
$fixture = Join-Path $PSScriptRoot 'fixtures\storage'
$injected = 'for(let i=0;i<100&&(location.pathname!=="__PATH__"||document.readyState!=="complete");i++) await new Promise(r=>setTimeout(r,50));if(location.pathname!=="__PATH__"||document.readyState!=="complete") throw new Error("Fixture navigation did not finish");return document.documentElement.dataset.zephiumStorageQualifier === "injected";'
$steps = @(
    @{load=$fixture}, @{view='popup'}, @{in='popup';popup=$true}, @{sleep=500},
    @{in='popup';eval='await chrome.storage.local.set({qaValue:"profile-one"});return true;'},
    @{navigate='$ORIGIN/enabled'}, @{sleep=500}, @{label='enabled';eval=$injected.Replace('__PATH__','/enabled')},
    @{enabled=$false}, @{navigate='$ORIGIN/disabled'}, @{sleep=500}, @{label='disabled';eval=$injected.Replace('__PATH__','/disabled')},
    @{enabled=$true}, @{navigate='$ORIGIN/reenabled'}, @{sleep=500}, @{label='reenabled';eval=$injected.Replace('__PATH__','/reenabled')},
    @{in='popup';popup=$true}, @{sleep=500},
    @{label='preserved';in='popup';eval='return (await chrome.storage.local.get("qaValue")).qaValue;'},
    @{view='other';profile='LabOther'}, @{label='other-empty';in='other';list=$true},
    @{in='other';load=$fixture}, @{in='other';popup=$true}, @{sleep=500},
    @{label='other-storage';in='other';eval='const before=await chrome.storage.local.get("qaValue");await chrome.storage.local.set({qaValue:"profile-two"});return {empty:before.qaValue===undefined};'},
    @{label='first-storage';in='popup';eval='return (await chrome.storage.local.get("qaValue")).qaValue;'},
    @{label='other-removed';in='other';remove=$true}, @{in='other';navigate='$ORIGIN/removed'}, @{sleep=500},
    @{label='removed-injection';in='other';eval=$injected.Replace('__PATH__','/removed')}, @{label='first-install';list=$true},
    @{navigate='$ORIGIN/still-enabled'}, @{sleep=500}, @{label='first-injection';eval=$injected.Replace('__PATH__','/still-enabled')}
)
$scenario = Join-Path $run 'scenario.json'
[IO.File]::WriteAllText($scenario, (ConvertTo-Json -InputObject $steps -Depth 10), [Text.UTF8Encoding]::new($false))
$stdout = Join-Path $run 'results.jsonl'
$probe = Start-Process -FilePath (Join-Path $repo 'target\debug\webext-lab-windows.exe') -ArgumentList @("`"$scenario`"", "`"$(Join-Path $run 'data')`"") -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError (Join-Path $run 'stderr.log')
$probe.Handle | Out-Null
if (-not $probe.WaitForExit(55000)) { $probe.Kill(); throw 'Lifecycle qualifier timed out.' }
if ($probe.ExitCode -ne 0) { throw 'Lifecycle qualifier failed.' }
$records = @(Get-Content $stdout | ForEach-Object { $_ | ConvertFrom-Json })
if (@($records | Where-Object { $_.kind -eq 'step' -and (-not $_.value.ok -or $_.value.result.PSObject.Properties['exceptionDetails']) }).Count) { throw 'Native lifecycle step failed.' }
function Result-For([string]$Label) { ($records | Where-Object { $_.value.label -eq $Label }).value.result }
$fixtureId = ($records | Where-Object { $_.kind -eq 'step' -and $_.value.index -eq 0 }).value.result.id
if (-not $fixtureId) { throw 'No native fixture identity.' }
foreach ($label in @('enabled','reenabled','first-injection')) {
    if ((Result-For $label).result.value -ne $true) { throw "Expected injection: $label" }
}
foreach ($label in @('disabled','removed-injection')) {
    if ((Result-For $label).result.value -ne $false) { throw "Unexpected injection: $label" }
}
foreach ($label in @('preserved','first-storage')) {
    if ((Result-For $label).result.value -ne 'profile-one') { throw "Storage changed: $label" }
}
if ((Result-For 'other-storage').result.value.empty -ne $true) { throw 'Storage crossed profiles.' }
foreach ($label in @('other-empty','other-removed')) {
    # WebView2's own component extensions (including its PDF viewer) remain.
    if (@(Result-For $label | Where-Object { $_.id -eq $fixtureId }).Count -ne 0) { throw "Unexpected fixture install: $label" }
}
$before = @(Result-For 'other-empty' | ForEach-Object { $_.id } | Sort-Object)
$after = @(Result-For 'other-removed' | ForEach-Object { $_.id } | Sort-Object)
if (($before -join ',') -ne ($after -join ',')) { throw 'Removal changed the native component inventory.' }
$first = @(Result-For 'first-install' | Where-Object { $_.id -eq $fixtureId })
if ($first.Count -ne 1 -or -not $first[0].enabled) { throw 'Removal affected the other profile.' }
Write-Output "Native lifecycle and profile isolation passed: $run"
