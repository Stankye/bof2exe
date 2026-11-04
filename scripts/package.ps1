param([string]$Output = '', [switch]$Archive)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    if ($env:OS -ne 'Windows_NT' -or -not [Environment]::Is64BitProcess) {
        throw 'Packaging requires Windows x64.'
    }
    & (Join-Path $PSScriptRoot 'check-repository.ps1')
    $packageMetadata = cargo +stable-x86_64-pc-windows-msvc metadata --locked --format-version 1 --no-deps
    if ($LASTEXITCODE -ne 0) { throw 'Package metadata failed' }
    $version = ($packageMetadata | ConvertFrom-Json).packages[0].version
    if ([string]::IsNullOrWhiteSpace($Output)) { $Output = "dist/bof2exe-$version-windows-x64" }
    $releaseDirectory = [IO.Path]::GetFullPath($Output)
    $archivePath = "$releaseDirectory.zip"
    if (Test-Path -LiteralPath $releaseDirectory) { throw "Output directory already exists: $releaseDirectory" }
    if ($Archive -and ((Test-Path -LiteralPath $archivePath) -or (Test-Path -LiteralPath "$archivePath.sha256"))) {
        throw "Archive output already exists: $archivePath"
    }
    cargo +stable-x86_64-pc-windows-msvc build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    & (Join-Path $PSScriptRoot 'prepare-fixtures.ps1')
    New-Item -ItemType Directory -Path $releaseDirectory | Out-Null
    Copy-Item -LiteralPath 'target/release/bof2exe.exe' -Destination $releaseDirectory
    Copy-Item -LiteralPath 'target/release/bof2exe-runner.exe' -Destination $releaseDirectory
    foreach ($name in @('README.md', 'CONTRIBUTING.md', 'CHANGELOG.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'THIRD_PARTY.md')) {
        Copy-Item -LiteralPath $name -Destination $releaseDirectory
    }
    foreach ($name in @('docs', 'include', 'examples')) {
        Copy-Item -LiteralPath $name -Destination $releaseDirectory -Recurse
    }
    $fixtureDestination = Join-Path $releaseDirectory 'tests'
    $scriptDestination = Join-Path $releaseDirectory 'scripts'
    New-Item -ItemType Directory -Path $fixtureDestination,$scriptDestination | Out-Null
    Copy-Item -LiteralPath 'tests/fixtures' -Destination $fixtureDestination -Recurse
    Copy-Item -LiteralPath 'scripts/prepare-fixtures.ps1' -Destination $scriptDestination
    $compiledFixtures = Join-Path $releaseDirectory 'target/fixtures'
    New-Item -ItemType Directory -Path $compiledFixtures -Force | Out-Null
    foreach ($name in @('hello.o', 'greeting.o', 'unwind.o', 'stack.o', 'link_main.o', 'link_helper.o', 'helpers.zip')) {
        Copy-Item -LiteralPath (Join-Path 'target/fixtures' $name) -Destination $compiledFixtures
    }
    & (Join-Path $releaseDirectory 'bof2exe.exe') build (Join-Path $releaseDirectory 'examples/hello.tool.toml') -o (Join-Path $releaseDirectory 'hello.exe') --docs (Join-Path $releaseDirectory 'hello.md')
    if ($LASTEXITCODE -ne 0) { throw 'Standalone example build failed' }
    $converterVersion = & (Join-Path $releaseDirectory 'bof2exe.exe') --version
    if ($LASTEXITCODE -ne 0 -or $converterVersion -ne "bof2exe $version") { throw 'Packaged version check failed' }
    & (Join-Path $releaseDirectory 'hello.exe') --help | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Standalone help check failed' }
    $greeting = & (Join-Path $releaseDirectory 'hello.exe') --name Alice --count 3
    if ($LASTEXITCODE -ne 0 -or ($greeting -join "`n").Trim() -ne 'Hello, Alice! Count: 3') { throw 'Standalone output check failed' }
    $metadataText = cargo +stable-x86_64-pc-windows-msvc metadata --locked --format-version 1 --filter-platform x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Dependency metadata failed' }
    $metadata = $metadataText | ConvertFrom-Json
    $licenseDirectory = Join-Path $releaseDirectory 'licenses'
    New-Item -ItemType Directory -Path $licenseDirectory | Out-Null
    $inventory = @()
    foreach ($package in $metadata.packages) {
        if ($package.name -eq 'bof2exe') { continue }
        $packageRoot = Split-Path -Parent $package.manifest_path
        $destination = Join-Path $licenseDirectory ($package.name + '-' + $package.version)
        New-Item -ItemType Directory -Path $destination | Out-Null
        $licenseFiles = @(Get-ChildItem -LiteralPath $packageRoot -File | Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE|UNLICENSE|COPYRIGHT|AUTHORS)' })
        if ($licenseFiles.Count -eq 0) { throw "No license text found for $($package.name)" }
        foreach ($file in $licenseFiles) { Copy-Item -LiteralPath $file.FullName -Destination $destination }
        $inventory += [pscustomobject]@{ name = $package.name; version = $package.version; license = $package.license; repository = $package.repository }
    }
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [IO.File]::WriteAllText((Join-Path $releaseDirectory 'dependencies.json'), ($inventory | ConvertTo-Json -Depth 5), $utf8)
    $checksums = foreach ($binary in @('bof2exe.exe', 'bof2exe-runner.exe', 'hello.exe')) {
        $checksum = (Get-FileHash -LiteralPath (Join-Path $releaseDirectory $binary) -Algorithm SHA256).Hash.ToLowerInvariant()
        "$checksum  $binary"
    }
    [IO.File]::WriteAllText((Join-Path $releaseDirectory 'SHA256SUMS'), (($checksums -join "`n") + "`n"), $utf8)
    # Cargo archives sometimes use pre-1980 timestamps, which ZIP cannot represent.
    Get-ChildItem -LiteralPath $releaseDirectory -Recurse -File | ForEach-Object {
        if ($_.LastWriteTime.Year -lt 1980) { $_.LastWriteTime = [datetime]'1980-01-01' }
    }
    if ($Archive) {
        Compress-Archive -LiteralPath $releaseDirectory -DestinationPath $archivePath
        $archiveHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
        [IO.File]::WriteAllText("$archivePath.sha256", "$archiveHash  $([IO.Path]::GetFileName($archivePath))`n", $utf8)
        Write-Host "Archive: $archivePath"
    }
    Write-Host "Packaged $releaseDirectory"
} finally { Pop-Location }
