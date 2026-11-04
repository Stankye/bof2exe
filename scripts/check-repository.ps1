param([switch]$ListFiles)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
$failures = New-Object 'Collections.Generic.List[string]'
$sourceFiles = @(Get-ChildItem -LiteralPath $projectRoot -Force -File)
foreach ($directory in @('.cargo', '.github', 'src', 'tests', 'include', 'examples', 'docs', 'scripts')) {
    $path = Join-Path $projectRoot $directory
    if (Test-Path -LiteralPath $path) {
        $sourceFiles += @(Get-ChildItem -LiteralPath $path -Force -Recurse -File)
    }
}
$required = @('README.md', 'CONTRIBUTING.md', 'CHANGELOG.md', 'LICENSE-MIT', 'LICENSE-APACHE',
    'THIRD_PARTY.md', 'Cargo.toml', 'Cargo.lock', '.gitignore', '.gitattributes',
    'docs/development.md', 'docs/releasing.md', '.github/workflows/ci.yml')
foreach ($name in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $projectRoot $name) -PathType Leaf)) {
        $failures.Add("Missing publication file: $name")
    }
}
$utf8 = New-Object Text.UTF8Encoding($false, $true)
$linkCount = 0
foreach ($file in $sourceFiles) {
    $relative = $file.FullName.Substring($projectRoot.Length + 1).Replace('\', '/')
    if ($relative -in @('.cargo/credentials', '.cargo/credentials.toml')) {
        $failures.Add("Local Cargo credentials in source selection: $relative")
        continue
    }
    if ($file.Name -match '^\.env(?:\.|$)' -and $file.Name -ne '.env.example') {
        $failures.Add("Local environment file in source selection: $relative")
        continue
    }
    if ($file.Extension -in @('.exe', '.dll', '.o', '.obj', '.lib', '.a', '.pdb', '.zip')) {
        $failures.Add("Build artifact in source selection: $relative")
        continue
    }
    try { $body = [IO.File]::ReadAllText($file.FullName, $utf8) }
    catch { $failures.Add("Source file is not UTF-8 text: $relative"); continue }
    if ($body -match '(?i)[A-Z]:[\\/]Users[\\/]|/(?:Users|home)/[^/\s]+/') {
        $failures.Add("Workstation path in source file: $relative")
    }
    if ($body -match '(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,}|AKIA[A-Z0-9]{16}|-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----)') {
        $failures.Add("Credential-like content requires review: $relative")
    }
    if ($file.Extension -eq '.ps1') {
        $tokens = $null
        $parseErrors = $null
        [void][Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$parseErrors)
        foreach ($errorItem in $parseErrors) {
            $failures.Add("PowerShell parse error in ${relative}:$($errorItem.Extent.StartLineNumber)")
        }
    }
    if ($file.Extension -eq '.md') {
        $prose = [regex]::Replace($body, '(?ms)^```[^\r\n]*\r?\n.*?^```[^\r\n]*$', '')
        foreach ($match in [regex]::Matches($prose, '\[[^\]\r\n]+\]\(([^)\r\n]+)\)')) {
            $target = $match.Groups[1].Value.Trim().Trim('<', '>')
            if ($target -match '^(?:https?://|mailto:|#)') { continue }
            $target = [uri]::UnescapeDataString(($target -split '[?#]', 2)[0])
            if (-not $target) { continue }
            $resolved = Join-Path $file.DirectoryName $target
            $linkCount++
            if (-not (Test-Path -LiteralPath $resolved)) {
                $failures.Add("Broken local link in ${relative}: $target")
            }
        }
    }
}
$manifest = [IO.File]::ReadAllText((Join-Path $projectRoot 'Cargo.toml'))
$version = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
$changelog = [IO.File]::ReadAllText((Join-Path $projectRoot 'CHANGELOG.md'))
if (-not $version -or $changelog -notmatch ('(?m)^## ' + [regex]::Escape($version) + '\s*$')) {
    $failures.Add('Cargo package version is missing from CHANGELOG.md')
}
if ($failures.Count -gt 0) {
    foreach ($failure in $failures) { Write-Output $failure }
    throw "Repository checks failed: $($failures.Count) issue(s)"
}
Write-Host "Repository checks passed: $($sourceFiles.Count) source files, $linkCount local documentation links, version $version."
if ($ListFiles) {
    $sourceFiles | Sort-Object FullName | ForEach-Object {
        $_.FullName.Substring($projectRoot.Length + 1).Replace('\', '/')
    }
}
