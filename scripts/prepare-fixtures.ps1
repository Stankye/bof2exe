$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    New-Item -ItemType Directory -Force target/fixtures | Out-Null
    foreach ($fixture in @('hello', 'greeting', 'unwind', 'stack', 'link_main', 'link_helper')) {
        $source = Join-Path 'tests/fixtures' ($fixture + '.rs')
        $object = Join-Path 'target/fixtures' ($fixture + '.o')
        rustc +stable-x86_64-pc-windows-msvc --edition=2021 --crate-type lib --emit=obj -C panic=abort -C opt-level=2 -C overflow-checks=off $source -o $object
        if ($LASTEXITCODE -ne 0) { throw "Fixture compilation failed: $fixture" }
    }
    Compress-Archive -LiteralPath 'target/fixtures/link_helper.o' -DestinationPath 'target/fixtures/helpers.zip' -Force
} finally { Pop-Location }
