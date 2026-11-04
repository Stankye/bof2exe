param(
    [Parameter(Mandatory = $true)][string]$Repository,
    [string]$ReportDirectory = 'target/external-bofs/reports',
    [switch]$InspectOnly
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$sourceRoot = (Resolve-Path -LiteralPath $Repository).Path
$cna = Join-Path $sourceRoot 'SA/SA.cna'
if (-not (Test-Path -LiteralPath $cna)) { throw "Missing upstream CNA: $cna" }
$reportRoot = [IO.Path]::GetFullPath($ReportDirectory)
$priorObjects = $env:BOF2EXE_TRUSTEDSEC_OBJECTS
$priorCna = $env:BOF2EXE_TRUSTEDSEC_CNA
$priorReports = $env:BOF2EXE_TRUSTEDSEC_REPORTS
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    $env:BOF2EXE_TRUSTEDSEC_OBJECTS = Join-Path $sourceRoot 'SA'
    $env:BOF2EXE_TRUSTEDSEC_CNA = $cna
    $env:BOF2EXE_TRUSTEDSEC_REPORTS = $reportRoot
    if ($InspectOnly) {
        cargo +stable-x86_64-pc-windows-msvc test --locked --test external_trustedsec all_upstream_x64 -- --ignored --nocapture
    } else {
        cargo +stable-x86_64-pc-windows-msvc test --locked --test external_trustedsec -- --ignored --nocapture
    }
    if ($LASTEXITCODE -ne 0) { throw "Upstream tests failed; reports: $reportRoot" }
    Write-Host "Upstream reports: $reportRoot"
} finally {
    $env:BOF2EXE_TRUSTEDSEC_OBJECTS = $priorObjects
    $env:BOF2EXE_TRUSTEDSEC_CNA = $priorCna
    $env:BOF2EXE_TRUSTEDSEC_REPORTS = $priorReports
    Pop-Location
}
