# Run from a Windows checkout of the reviewed integration commit.
# Requires the repository's Rust/MSVC, pnpm, Node and WebView2 prerequisites.
# Builds an isolated debug candidate. Does not launch it or touch a release profile.
$ErrorActionPreference = 'Stop'
$repository = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
Push-Location $repository
function Invoke-Checked {
    param([string]$Program, [string[]]$Arguments)
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE" }
}
try {
    if (-not $IsWindows -and $env:OS -ne 'Windows_NT') { throw 'Run on Windows.' }
    Invoke-Checked 'cargo' @('test', '--manifest-path', 'vendor/wry/Cargo.toml', '--lib', '--locked')
    Invoke-Checked 'cargo' @('test', '-p', 'zephium-core', '-p', 'zephium-store', '-p', 'zephium-engine', '--lib', '--locked', '--', '--test-threads=1')
    Invoke-Checked 'cargo' @('test', '-p', 'zephium-desktop', '--locked', '--', '--test-threads=1')
    Invoke-Checked 'pnpm.cmd' @('-C', 'frame', 'check')
    Invoke-Checked 'pnpm.cmd' @('-C', 'frame', 'test:component')
    Invoke-Checked 'pnpm.cmd' @('-C', 'frame', 'build')
    Invoke-Checked 'cargo' @('xtask', 'check-frame-styles')
    Invoke-Checked './desktop/node_modules/.bin/tauri.cmd' @('build', '--debug', '--no-bundle', '--features', 'file-workflows-qa', '--config', 'desktop/tauri.files-qa.windows.conf.json', '--no-sign')
    Write-Host 'Candidate built. Follow docs/file-workflows-windows-qa.md before release qualification.'
} finally {
    Pop-Location
}
