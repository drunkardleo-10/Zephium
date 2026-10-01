param(
    [ValidateRange(3, 20)][int]$Samples = 5,
    [ValidateSet(0, 1, 3)][int[]]$Counts = @(0, 1, 3),
    [switch]$LowMemory,
    [string]$Label = 'local',
    [string]$LabBinary
)
$ErrorActionPreference = 'Stop'
if ($Label -notmatch '^[a-zA-Z0-9-]+$') { throw 'Use a simple filename label.' }
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $LabBinary) { $LabBinary = Join-Path $repo 'target\debug\webext-lab-windows.exe' }
$LabBinary = (Resolve-Path -LiteralPath $LabBinary).Path
$results = Join-Path $repo ('target\webext-page-performance\' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + $Label)
New-Item -ItemType Directory -Path $results -Force | Out-Null
$suite = Get-Content (Join-Path $repo 'crates\zephium-webext-macos\suite.json') -Raw | ConvertFrom-Json
$names = @('Dark Reader', 'Grammarly', 'Bitwarden')
$utf8 = [Text.UTF8Encoding]::new($false)
$instrument = @'
globalThis.cost = {fetches:0, decodes:0, snapshots:0};
const fetchOriginal = fetch, bitmapOriginal = createImageBitmap;
const messageOriginal = chrome.runtime.sendMessage;
chrome.runtime.sendMessage = function (...args) {
  if (args[0]?.__zephiumActionSnapshot === true) cost.snapshots++;
  return Reflect.apply(messageOriginal, this, args);
};
globalThis.fetch = (...args) => { cost.fetches++; return fetchOriginal(...args); };
globalThis.createImageBitmap = (...args) => { cost.decodes++; return bitmapOriginal(...args); };
globalThis.measureAction = () => new Promise((resolve, reject) => {
  const start = performance.now(), original = chrome.webview.postMessage.bind(chrome.webview);
  const timeout = setTimeout(() => { chrome.webview.postMessage = original; reject(new Error('action timeout')); }, 5000);
  chrome.webview.postMessage = message => {
    original(message);
    const value = JSON.parse(message);
    if (value.kind === 'action' && value.windowId === $HUMAN_WINDOW) {
      clearTimeout(timeout); chrome.webview.postMessage = original;
      resolve({milliseconds:performance.now()-start, iconBytes:value.icon?.length || 0, ...cost});
    }
  };
  window.__zephiumRefresh($HUMAN_WINDOW);
});
return true;
'@
$measurePage = @'
if (document.readyState !== 'complete' || document.querySelectorAll('article').length !== 600) throw new Error('fixture incomplete');
const nav = performance.getEntriesByType('navigation')[0];
const paint = performance.getEntriesByName('first-contentful-paint')[0];
return {domContentLoadedMs:nav.domContentLoadedEventEnd, loadMs:nav.loadEventEnd,
  firstContentfulPaintMs:paint?.startTime ?? null, nodes:document.querySelectorAll('*').length,
  longTaskMs:longTasks.reduce((sum, task) => sum + task.duration, 0),
  darkReader:!!document.querySelector('style.darkreader'),
  heapUsed:performance.memory?.usedJSHeapSize ?? null};
'@
foreach ($count in $Counts) {
    $steps = [Collections.Generic.List[object]]::new()
    $steps.Add(@{human_visible=$true; navigate='$ORIGIN/first'})
    for ($index = 0; $index -lt $count; $index++) {
        $entry = $suite | Where-Object name -eq $names[$index]
        $package = Join-Path $repo "target\webext-suite\$($entry.id).crx"
        if (-not (Test-Path -LiteralPath $package)) { throw "Missing signed package: $package" }
        $steps.Add(@{load=$package; prepare_windows=$true})
        $steps.Add(@{view="manager$index"})
        if ($LowMemory) { $steps.Add(@{in="manager$index"; low_memory=$true}) }
        $steps.Add(@{in="manager$index"; navigate='chrome-extension://$ID/zephium-windows-host/host.html'})
        $steps.Add(@{sleep=1500})
        $steps.Add(@{in="manager$index"; capture_human=$true})
        $steps.Add(@{in="manager$index"; eval=$instrument})
    }
    $steps.Add(@{sleep=3000})
    for ($index = 0; $index -lt $count; $index++) {
        $steps.Add(@{in="manager$index"; label="steady-$index"; eval='const start=performance.now(), before={...cost}; for(let i=0;i<20;i++) await measureAction(); return {milliseconds:performance.now()-start, fetches:cost.fetches-before.fetches, decodes:cost.decodes-before.decodes, snapshots:cost.snapshots-before.snapshots};'})
    }
    # Sample zero is an explicit warm-up; retain it separately from warm medians.
    for ($sample = 0; $sample -le $Samples; $sample++) {
        $steps.Add(@{navigate=('$ORIGIN/performance?sample=' + $sample)})
        for ($index = 0; $index -lt $count; $index++) {
            $steps.Add(@{in="manager$index"; eval='return await measureAction();'; label="action-$sample-$index"})
        }
        $steps.Add(@{sleep=2500})
        $steps.Add(@{eval=$measurePage; label="page-$sample"})
    }
    $scenario = Join-Path $results "$count.json"
    [IO.File]::WriteAllText($scenario, (ConvertTo-Json -InputObject @($steps.ToArray()) -Depth 12), $utf8)
    $stdout = Join-Path $results "$count.jsonl"
    $process = Start-Process -FilePath $LabBinary -ArgumentList @("`"$scenario`"", "`"$(Join-Path $results "$count.data")`"") -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError (Join-Path $results "$count.stderr.log")
    $processHandle = $process.Handle
    $deadline = [DateTime]::UtcNow.AddSeconds(120 + 15 * $Samples)
    while (-not $process.WaitForExit(1000)) {
        if ([DateTime]::UtcNow -gt $deadline) { $process.Kill(); throw "Performance lab timed out: $count" }
    }
    if ($process.ExitCode -ne 0) { throw "Performance lab failed: $count" }
    $records = @(Get-Content $stdout | ForEach-Object { $_ | ConvertFrom-Json })
    if (@($records | Where-Object { $_.kind -eq 'step' -and (-not $_.value.ok -or $_.value.result.exceptionDetails) }).Count) { throw "Performance step failed: $stdout" }
    foreach ($record in $records | Where-Object { $_.value.label -like 'page-*' }) {
        $value = $record.value.result.result.value
        if ($null -eq $value.firstContentfulPaintMs) { throw 'No visible paint sample; keep the lab window visible.' }
        if ($count -gt 0 -and -not $value.darkReader) { throw 'Dark Reader did not modify the fixture.' }
        [pscustomobject]@{extensions=$count; sample=$record.value.label; dclMs=$value.domContentLoadedMs; loadMs=$value.loadMs; fcpMs=$value.firstContentfulPaintMs; longTaskMs=$value.longTaskMs; heapUsed=$value.heapUsed; nodes=$value.nodes} | Export-Csv (Join-Path $results 'pages.csv') -NoTypeInformation -Append
    }
    foreach ($record in $records | Where-Object { $_.value.label -like 'action-*' }) {
        $value = $record.value.result.result.value
        [pscustomobject]@{extensions=$count; sample=$record.value.label; milliseconds=$value.milliseconds; iconBytes=$value.iconBytes; fetches=$value.fetches; decodes=$value.decodes; workerSnapshots=$value.snapshots} | Export-Csv (Join-Path $results 'actions.csv') -NoTypeInformation -Append
    }
    foreach ($record in $records | Where-Object { $_.value.label -like 'steady-*' }) {
        $value = $record.value.result.result.value
        [pscustomobject]@{extensions=$count; manager=$record.value.label; milliseconds=$value.milliseconds; fetches=$value.fetches; decodes=$value.decodes; workerSnapshots=$value.snapshots} | Export-Csv (Join-Path $results 'steady.csv') -NoTypeInformation -Append
    }
    Write-Output "Completed $count extensions"
}
Write-Output "Controlled local-page results (not real-site network timings): $results"
