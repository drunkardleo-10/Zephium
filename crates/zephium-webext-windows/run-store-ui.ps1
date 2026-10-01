$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$run = Join-Path $repo ('target\webext-store-ui\' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $run -Force | Out-Null
$inspect = @'
const controls=[...document.querySelectorAll('[jscontroller="ri2s0b"]')];
return {host:location.hostname,found:controls.length,
  hidden:controls.every(el=>getComputedStyle(el).display==='none'),
  visibleOtherButtons:[...document.querySelectorAll('button')].filter(el=>!el.closest('[jscontroller="ri2s0b"]')&&el.getClientRects().length>0).length};
'@
$steps = @(
    @{chrome_store_ui=$true;navigate='$ORIGIN/form'}, @{sleep=500},
    @{label='ordinary-page';eval='const control=document.createElement("div");control.setAttribute("jscontroller","ri2s0b");control.textContent="Unrelated control";document.body.append(control);return {visible:getComputedStyle(control).display!=="none"};'},
    @{navigate='https://chromewebstore.google.com/detail/nngceckbapebfimnlniiiahkandclblb?hl=en'}, @{sleep=4000},
    @{eval='if(location.hostname==="consent.google.com") document.querySelector("button[aria-label=\"Reject all\"]")?.click();return "consent checked";'}, @{sleep=5000},
    @{label='english-listing';eval=$inspect},
    @{navigate='https://chromewebstore.google.com/detail/eimadpbcbfnmbkopoojfekhnkhdbieeh?hl=pl'}, @{sleep=5000},
    @{label='polish-listing';eval=$inspect}
)
$scenario = Join-Path $run 'scenario.json'
[IO.File]::WriteAllText($scenario, (ConvertTo-Json -InputObject $steps -Depth 10), [Text.UTF8Encoding]::new($false))
$stdout = Join-Path $run 'results.jsonl'
$probe = Start-Process -FilePath (Join-Path $repo 'target\debug\webext-lab-windows.exe') -ArgumentList @("`"$scenario`"", "`"$(Join-Path $run 'data')`"") -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError (Join-Path $run 'stderr.log')
$probe.Handle | Out-Null
if (-not $probe.WaitForExit(55000)) { $probe.Kill(); throw 'Store UI qualifier timed out.' }
if ($probe.ExitCode -ne 0) { throw 'Store UI qualifier failed.' }
$records = @(Get-Content $stdout | ForEach-Object { $_ | ConvertFrom-Json })
foreach ($label in @('english-listing', 'polish-listing')) {
    $result = ($records | Where-Object { $_.value.label -eq $label }).value.result.result.value
    if ($result.host -ne 'chromewebstore.google.com' -or $result.found -lt 1 -or -not $result.hidden -or $result.visibleOtherButtons -lt 1) { throw "Store UI check failed: $label. Inspect the current store markup or network result." }
}
$ordinary = ($records | Where-Object { $_.value.label -eq 'ordinary-page' }).value.result.result.value
if (-not $ordinary.visible) { throw 'An unrelated page was modified.' }
Write-Output "Store UI qualifier passed: $run"
