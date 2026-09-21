$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repositoryRoot = Split-Path $PSScriptRoot -Parent
Push-Location $repositoryRoot
try {
    $vendorRoot = [IO.Path]::GetFullPath((Join-Path $repositoryRoot 'vendor/libsqlite3-sys'))
    $checksums = Get-Content -LiteralPath 'vendor/checksums.json' -Raw | ConvertFrom-Json
    $actualFiles = @(Get-ChildItem -LiteralPath $vendorRoot -Recurse -File | ForEach-Object {
        [IO.Path]::GetRelativePath($vendorRoot, $_.FullName).Replace('\', '/')
    })
    if (@(Compare-Object $actualFiles @($checksums.path)).Count -ne 0) {
        throw 'Native dependency file inventory changed.'
    }
    foreach ($entry in $checksums) {
        $path = [IO.Path]::GetFullPath((Join-Path $vendorRoot $entry.path))
        if (-not $path.StartsWith($vendorRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::Ordinal)) {
            throw 'Native dependency manifest contains an out-of-scope path.'
        }
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.sha256) {
            throw "Native dependency checksum mismatch: $($entry.path)"
        }
    }
    $commands = @(
        @('fmt', '--all', '--', '--check'),
        @('test', '--workspace', '--locked', '--', '--test-threads=2'),
        @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings'),
        @('build', '--workspace', '--locked'),
        @('audit', '--deny', 'warnings')
    )
    foreach ($arguments in $commands) {
        & cargo @arguments
        if ($LASTEXITCODE -ne 0) {
            throw "Verification failed: cargo $($arguments -join ' ')"
        }
    }
} finally {
    Pop-Location
}
