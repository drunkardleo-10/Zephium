param([switch]$Build)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$qaDir = Join-Path $repo 'target\webext-qa'
$qaExe = Join-Path $qaDir 'Zephium Extensions QA.exe'
$developmentExe = Join-Path $repo 'target\debug\zephium-desktop.exe'
$running = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
    -not $_.HasExited -and ($_.Path -eq $qaExe -or $_.Path -eq $developmentExe)
})
if ($running.Count) { throw 'Close this checkout''s QA/development app before building or launching QA.' }
if ($Build) {
    # Prefer this checkout's pinned tools when present; ordinary PATH also works.
    foreach ($relative in @('target\windows-tools\pnpm\node_modules\.bin', 'target\windows-tools\node-v24.18.0-win-x64')) {
        $toolPath = Join-Path $repo $relative
        if (Test-Path -LiteralPath $toolPath) { $env:PATH = $toolPath + ';' + $env:PATH }
    }
    Push-Location -LiteralPath $repo
    try {
        & pnpm.cmd -C desktop exec tauri build --debug --no-bundle --features webext-qa --config tauri.webext-qa.conf.json
        if ($LASTEXITCODE -ne 0) { throw 'QA build failed.' }
        $metadata = (Get-Item -LiteralPath $developmentExe).VersionInfo
        if ($metadata.ProductName -ne 'Zephium Extensions QA') { throw 'Refusing a binary without the QA identity.' }
        New-Item -ItemType Directory -Path $qaDir -Force | Out-Null
        Copy-Item -LiteralPath $developmentExe -Destination $qaExe -Force
        & git rev-parse HEAD | Set-Content -LiteralPath (Join-Path $qaDir 'revision.txt')
    } finally { Pop-Location }
}
if (-not (Test-Path -LiteralPath $qaExe)) { throw 'Run this script with -Build first.' }
if ((Get-Item -LiteralPath $qaExe).VersionInfo.ProductName -ne 'Zephium Extensions QA') {
    throw 'Refusing a binary without the QA identity.'
}
$process = Start-Process -FilePath $qaExe -WorkingDirectory $repo -PassThru
Write-Output "Started Zephium Extensions QA (PID $($process.Id))."
Write-Output 'Data: %APPDATA%\app.zephium.webext-qa'
