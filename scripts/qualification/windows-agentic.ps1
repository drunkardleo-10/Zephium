[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("CollectNonDebugger", "ReviewAfterDebugger")]
    [string] $Phase,

    [switch] $AuthorizedPhysicalWindows
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = "Stop"

function Invoke-Cargo {
    param(
        [Parameter(Mandatory = $true)]
        [string[]] $Arguments
    )

    & cargo --offline @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo command failed with exit code $LASTEXITCODE"
    }
}

function Get-SourceRevision {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Repository
    )

    $lines = @(& git -C $Repository rev-parse --verify HEAD)
    if ($LASTEXITCODE -ne 0 -or $lines.Count -ne 1) {
        throw "the qualification checkout has no exact Git revision"
    }
    $revision = $lines[0].Trim()
    if ($revision -cnotmatch '^[0-9a-f]{40}$') {
        throw "the qualification checkout revision is not canonical"
    }
    return $revision
}

function Get-CargoTargetDirectory {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Repository
    )

    $metadataLines = @(& cargo --offline metadata --locked --format-version 1 --no-deps --manifest-path (Join-Path $Repository "Cargo.toml"))
    if ($LASTEXITCODE -ne 0 -or $metadataLines.Count -eq 0) {
        throw "the qualification Cargo target directory is unavailable"
    }
    $metadata = ($metadataLines -join "`n") | ConvertFrom-Json
    if ($null -eq $metadata -or [string]::IsNullOrWhiteSpace([string] $metadata.target_directory)) {
        throw "the qualification Cargo target directory is invalid"
    }
    return [System.IO.Path]::GetFullPath([string] $metadata.target_directory)
}

function Assert-CleanCheckout {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Repository
    )

    $status = @(& git -C $Repository status --porcelain=v1 --untracked-files=all)
    if ($LASTEXITCODE -ne 0) {
        throw "the qualification checkout status is unavailable"
    }
    if ($status.Count -ne 0) {
        throw "physical qualification requires a clean exact checkout"
    }
}

function Assert-DirectDirectory {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,

        [Parameter(Mandatory = $true)]
        [bool] $Create
    )

    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if ($null -eq $item -and $Create) {
        [System.IO.Directory]::CreateDirectory($Path) | Out-Null
        $item = Get-Item -LiteralPath $Path -Force
    }
    if ($null -eq $item -or -not $item.PSIsContainer) {
        throw "the fixed evidence directory is missing or not a directory"
    }
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "the fixed evidence directory must not be a reparse point"
    }
}

function Assert-DirectFile {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if ($null -eq $item -or $item.PSIsContainer) {
        throw "one required qualification record is missing or not a file"
    }
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "qualification records must not be reparse points"
    }
}

function Write-SourceStamp {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,

        [Parameter(Mandatory = $true)]
        [string] $Revision
    )

    $encoding = New-Object System.Text.UTF8Encoding($false)
    $bytes = $encoding.GetBytes("$Revision`n")
    $stream = [System.IO.File]::Open(
        $Path,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::None
    )
    try {
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    }
    finally {
        $stream.Dispose()
    }
}

function Get-FileSha256 {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    Assert-DirectFile -Path $Path
    $hash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    if ($hash -notmatch '^[0-9a-fA-F]{64}$') {
        throw "the qualification binary hash is not canonical"
    }
    return $hash.ToLowerInvariant()
}

function Write-BinaryHashStamp {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,

        [Parameter(Mandatory = $true)]
        [string] $Hash
    )

    if ($Hash -cnotmatch '^[0-9a-f]{64}$') {
        throw "the qualification binary hash is not canonical"
    }
    $encoding = New-Object System.Text.UTF8Encoding($false)
    $bytes = $encoding.GetBytes("$Hash`n")
    $stream = [System.IO.File]::Open(
        $Path,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::None
    )
    try {
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    }
    finally {
        $stream.Dispose()
    }
}

function Assert-ExactDirectoryEntries {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Directory,

        [Parameter(Mandatory = $true)]
        [string[]] $ExpectedNames
    )

    $actual = @(Get-ChildItem -LiteralPath $Directory -Force | ForEach-Object { $_.Name } | Sort-Object)
    $expected = @($ExpectedNames | Sort-Object)
    if ($actual.Count -ne $expected.Count) {
        throw "the fixed evidence directory contains an unexpected entry set"
    }
    for ($index = 0; $index -lt $expected.Count; $index += 1) {
        if ($actual[$index] -cne $expected[$index]) {
            throw "the fixed evidence directory contains an unexpected entry set"
        }
    }
}

if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
    throw "physical Windows qualification must run on Windows"
}
if ([System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne [System.Runtime.InteropServices.Architecture]::X64) {
    throw "physical Windows qualification requires an x86-64 process"
}

$rustcVersion = @(& rustc -vV)
if ($LASTEXITCODE -ne 0 -or -not ($rustcVersion -ccontains "host: x86_64-pc-windows-msvc")) {
    throw "physical Windows qualification requires the x86_64-pc-windows-msvc host toolchain"
}

$repository = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$reportedRoot = @(& git -C $repository rev-parse --show-toplevel)
if ($LASTEXITCODE -ne 0 -or $reportedRoot.Count -ne 1) {
    throw "the qualification repository root is unavailable"
}
$reportedRoot = [System.IO.Path]::GetFullPath($reportedRoot[0].Trim())
if ($reportedRoot -ne $repository) {
    throw "the qualification script must run from its owning checkout"
}

Assert-CleanCheckout -Repository $repository
$sourceRevision = Get-SourceRevision -Repository $repository
$evidenceDirectory = Join-Path $repository "eval\agentic-browsing\local-results"
$sourceStampName = "windows-qualification-source-v1.txt"
$debuggerBinaryHashStampName = "windows-semantic-debugger-binary-sha256-v1.txt"
$inputRecords = @(
    "windows-hidden-fixed-dom.jsonl",
    "windows-hidden-hwnd.jsonl",
    "windows-hidden-cdp.jsonl",
    "windows-visible-background-all.jsonl"
)
$semanticRecords = @(
    "windows-semantic-fixed-documents.jsonl",
    "windows-semantic-redirect-lifecycle.jsonl",
    "windows-semantic-location-replacement.jsonl",
    "windows-semantic-suspend-resume.jsonl",
    "windows-semantic-event-flood.jsonl",
    "windows-semantic-renderer-loss.jsonl",
    "windows-semantic-debugger-coexistence.jsonl"
)
$inputSummary = "windows-review-summary-v2.json"
$semanticSummary = "windows-semantic-review-summary-v5.json"

Push-Location $repository
try {
    if ($Phase -eq "CollectNonDebugger") {
        if (-not $AuthorizedPhysicalWindows.IsPresent) {
            throw "collection requires -AuthorizedPhysicalWindows after external authorization"
        }
        Assert-DirectDirectory -Path $evidenceDirectory -Create $true
        Assert-ExactDirectoryEntries -Directory $evidenceDirectory -ExpectedNames @()

        Invoke-Cargo -Arguments @("xtask", "check-agentic-probe-boundary")
        Invoke-Cargo -Arguments @("test", "--locked", "-p", "zephium-agentic", "--features", "probe-harness")
        Invoke-Cargo -Arguments @("test", "--locked", "-p", "zephium-engine", "--features", "agentic-browser", "--lib")
        Invoke-Cargo -Arguments @("check", "--locked", "-p", "zephium-engine", "--features", "agentic-browser")
        Invoke-Cargo -Arguments @("clippy", "--locked", "-p", "zephium-engine", "--features", "native-agentic-input-probe", "--bin", "windows-agentic-input-probe")
        Invoke-Cargo -Arguments @("build", "--locked", "-p", "zephium-engine", "--features", "native-agentic-input-probe", "--bin", "windows-agentic-input-probe")
        Invoke-Cargo -Arguments @("clippy", "--locked", "-p", "zephium-engine", "--features", "native-agentic-semantic-probe", "--bin", "windows-agentic-semantic-probe")
        Invoke-Cargo -Arguments @("build", "--locked", "-p", "zephium-engine", "--features", "native-agentic-semantic-probe", "--bin", "windows-agentic-semantic-probe")

        $cargoTargetDirectory = Get-CargoTargetDirectory -Repository $repository
        Assert-DirectDirectory -Path $cargoTargetDirectory -Create $false
        $semanticProbeBinary = Join-Path $cargoTargetDirectory "debug\windows-agentic-semantic-probe.exe"
        $preflightSemanticProbeBinaryHash = Get-FileSha256 -Path $semanticProbeBinary

        # Tests/build scripts must not alter the qualified source or pre-create
        # a result. Bind the stamp only after every offline/native preflight is
        # green so a failed compile also leaves the create-new evidence path
        # reusable without manual cleanup.
        Assert-CleanCheckout -Repository $repository
        $preflightRevision = Get-SourceRevision -Repository $repository
        if ($preflightRevision -cne $sourceRevision) {
            throw "the qualification source changed during preflight"
        }
        Assert-ExactDirectoryEntries -Directory $evidenceDirectory -ExpectedNames @()
        Write-SourceStamp -Path (Join-Path $evidenceDirectory $sourceStampName) -Revision $sourceRevision

        Invoke-Cargo -Arguments @("run", "--locked", "-p", "zephium-engine", "--features", "native-agentic-input-probe", "--bin", "windows-agentic-input-probe", "--", "--ci-hidden-fixed-dom", "--evidence-directory", "eval/agentic-browsing/local-results")
        Invoke-Cargo -Arguments @("run", "--locked", "-p", "zephium-engine", "--features", "native-agentic-input-probe", "--bin", "windows-agentic-input-probe", "--", "--ci-hidden-hwnd", "--evidence-directory", "eval/agentic-browsing/local-results")
        Invoke-Cargo -Arguments @("run", "--locked", "-p", "zephium-engine", "--features", "native-agentic-input-probe", "--bin", "windows-agentic-input-probe", "--", "--ci-hidden-cdp", "--evidence-directory", "eval/agentic-browsing/local-results")
        Invoke-Cargo -Arguments @("run", "--locked", "-p", "zephium-engine", "--features", "native-agentic-input-probe", "--bin", "windows-agentic-input-probe", "--", "--visible-background-windows-all", "--evidence-directory", "eval/agentic-browsing/local-results")
        Invoke-Cargo -Arguments @("run", "--locked", "-p", "zephium-agentic", "--features", "probe-harness", "--bin", "windows-agentic-input-evidence-review", "--", "--directory", "eval/agentic-browsing/local-results", "--write-summary")

        Invoke-Cargo -Arguments @("run", "--locked", "-p", "zephium-engine", "--features", "native-agentic-semantic-probe", "--bin", "windows-agentic-semantic-probe", "--", "--ci-hidden-fixed-documents", "--evidence-directory", "eval/agentic-browsing/local-results")
        Invoke-Cargo -Arguments @("run", "--locked", "-p", "zephium-engine", "--features", "native-agentic-semantic-probe", "--bin", "windows-agentic-semantic-probe", "--", "--ci-hidden-redirect-lifecycle", "--evidence-directory", "eval/agentic-browsing/local-results")
        Invoke-Cargo -Arguments @("run", "--locked", "-p", "zephium-engine", "--features", "native-agentic-semantic-probe", "--bin", "windows-agentic-semantic-probe", "--", "--ci-hidden-location-replacement", "--evidence-directory", "eval/agentic-browsing/local-results")
        Invoke-Cargo -Arguments @("run", "--locked", "-p", "zephium-engine", "--features", "native-agentic-semantic-probe", "--bin", "windows-agentic-semantic-probe", "--", "--ci-hidden-suspend-resume", "--evidence-directory", "eval/agentic-browsing/local-results")
        Invoke-Cargo -Arguments @("run", "--locked", "-p", "zephium-engine", "--features", "native-agentic-semantic-probe", "--bin", "windows-agentic-semantic-probe", "--", "--ci-hidden-event-flood", "--evidence-directory", "eval/agentic-browsing/local-results")
        Invoke-Cargo -Arguments @("run", "--locked", "-p", "zephium-engine", "--features", "native-agentic-semantic-probe", "--bin", "windows-agentic-semantic-probe", "--", "--ci-hidden-renderer-loss", "--evidence-directory", "eval/agentic-browsing/local-results")
        Invoke-Cargo -Arguments @("build", "--locked", "-p", "zephium-engine", "--features", "native-agentic-semantic-probe", "--bin", "windows-agentic-semantic-probe")

        # The debugger launch is intentionally manual and separately
        # authorized. Bind it to the exact executable produced by this source-
        # checked collection rather than assuming Cargo's default target path.
        $semanticProbeBinaryHash = Get-FileSha256 -Path $semanticProbeBinary
        if ($semanticProbeBinaryHash -cne $preflightSemanticProbeBinaryHash) {
            throw "the debugger binary changed during non-debugger collection"
        }
        Write-BinaryHashStamp -Path (Join-Path $evidenceDirectory $debuggerBinaryHashStampName) -Hash $semanticProbeBinaryHash

        $expectedBeforeDebugger = @($sourceStampName, $debuggerBinaryHashStampName) + $inputRecords + @($inputSummary) + $semanticRecords[0..5]
        Assert-ExactDirectoryEntries -Directory $evidenceDirectory -ExpectedNames $expectedBeforeDebugger
        Write-Host "Non-debugger evidence is complete for source revision $sourceRevision."
        Write-Host "Launch this exact source-bound binary under the separately authorized debugger:"
        Write-Host $semanticProbeBinary
        Write-Host "Use exactly these arguments:"
        Write-Host "--ci-hidden-debugger-coexistence --evidence-directory eval/agentic-browsing/local-results"
        Write-Host "Then run this script with -Phase ReviewAfterDebugger."
    }
    else {
        Assert-DirectDirectory -Path $evidenceDirectory -Create $false
        $sourceStamp = Join-Path $evidenceDirectory $sourceStampName
        Assert-DirectFile -Path $sourceStamp
        $recordedSourceStamp = [System.IO.File]::ReadAllText($sourceStamp)
        if ($recordedSourceStamp -cnotmatch '^[0-9a-f]{40}\n$') {
            throw "the qualification source stamp is invalid"
        }
        $recordedRevision = $recordedSourceStamp.Substring(0, 40)
        if ($recordedRevision -cne $sourceRevision) {
            throw "the debugger/review phase checkout differs from collection"
        }
        $debuggerBinaryHashStamp = Join-Path $evidenceDirectory $debuggerBinaryHashStampName
        Assert-DirectFile -Path $debuggerBinaryHashStamp
        $recordedDebuggerBinaryHashStamp = [System.IO.File]::ReadAllText($debuggerBinaryHashStamp)
        if ($recordedDebuggerBinaryHashStamp -cnotmatch '^[0-9a-f]{64}\n$') {
            throw "the debugger binary hash stamp is invalid"
        }
        $recordedDebuggerBinaryHash = $recordedDebuggerBinaryHashStamp.Substring(0, 64)
        $cargoTargetDirectory = Get-CargoTargetDirectory -Repository $repository
        Assert-DirectDirectory -Path $cargoTargetDirectory -Create $false
        $semanticProbeBinary = Join-Path $cargoTargetDirectory "debug\windows-agentic-semantic-probe.exe"
        $currentDebuggerBinaryHash = Get-FileSha256 -Path $semanticProbeBinary
        if ($currentDebuggerBinaryHash -cne $recordedDebuggerBinaryHash) {
            throw "the debugger binary differs from the collection-phase build"
        }
        foreach ($name in $inputRecords + @($inputSummary) + $semanticRecords) {
            Assert-DirectFile -Path (Join-Path $evidenceDirectory $name)
        }
        $expectedBeforeReview = @($sourceStampName, $debuggerBinaryHashStampName) + $inputRecords + @($inputSummary) + $semanticRecords
        Assert-ExactDirectoryEntries -Directory $evidenceDirectory -ExpectedNames $expectedBeforeReview
        if (Test-Path -LiteralPath (Join-Path $evidenceDirectory $semanticSummary)) {
            throw "the semantic review summary already exists"
        }
        Invoke-Cargo -Arguments @("run", "--locked", "-p", "zephium-agentic", "--features", "probe-harness", "--bin", "windows-agentic-semantic-evidence-review", "--", "--directory", "eval/agentic-browsing/local-results", "--write-summary")
        Assert-DirectFile -Path (Join-Path $evidenceDirectory $semanticSummary)
        $expectedAfterReview = $expectedBeforeReview + @($semanticSummary)
        Assert-ExactDirectoryEntries -Directory $evidenceDirectory -ExpectedNames $expectedAfterReview
        Write-Host "Closed input and semantic summaries are ready for human review."
    }
}
finally {
    Pop-Location
}
