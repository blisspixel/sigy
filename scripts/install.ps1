& {
$previousLocation = Get-Location
$previousEnvironment = @{}
foreach ($name in @('Path','SIGY_GIT_COMMIT','GIT_TERMINAL_PROMPT','CARGO_HOME','CARGO_INSTALL_ROOT')) {
    $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
$installLock = $null
try {
# Install sigy from https://github.com/blisspixel/sigy. This is not cargo verify.
$ErrorActionPreference = "Stop"
if (Get-Variable PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}
$env:GIT_TERMINAL_PROMPT = "0"
$env:GIT_CONFIG_NOSYSTEM = "1"
Remove-Item Env:GIT_CONFIG_PARAMETERS -ErrorAction SilentlyContinue
Remove-Item Env:GIT_CONFIG_COUNT -ErrorAction SilentlyContinue
Remove-Item Env:GIT_SSH_COMMAND -ErrorAction SilentlyContinue
Remove-Item Env:GIT_ASKPASS -ErrorAction SilentlyContinue
Remove-Item Env:SSH_ASKPASS -ErrorAction SilentlyContinue
Remove-Item Env:GIT_EXEC_PATH -ErrorAction SilentlyContinue
$gitFlags = @('-c', 'core.abbrev=40', '-c', 'core.fsmonitor=', '-c', 'core.hooksPath=NUL', '-c', 'http.followRedirects=false', '-c', 'protocol.version=2', '-c', 'transfer.fsckObjects=true')

foreach ($name in @('CARGO_HOME','CARGO_INSTALL_ROOT')) {
    $value = [Environment]::GetEnvironmentVariable($name, 'Process')
    if ($value) {
        [Environment]::SetEnvironmentVariable($name,
            [IO.Path]::GetFullPath([IO.Path]::Combine($previousLocation.ProviderPath, $value)), 'Process')
    }
}

$repo = "https://github.com/blisspixel/sigy.git"
$metadata = Join-Path $env:USERPROFILE ".sigy"
[void][IO.Directory]::CreateDirectory($metadata)
try { $installLock = [IO.File]::Open((Join-Path $metadata "install.lock"), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
catch { throw "Another Sigy installation is active, or its lock cannot be opened." }

# Fence any previous asynchronous handoff while this installer owns the shared lock.
$updateStatus = Join-Path $metadata 'update-status.json'
if (Test-Path -LiteralPath $updateStatus) {
    $receiptStream = [IO.File]::OpenRead($updateStatus)
    try {
        $receiptBytes = New-Object byte[] 8193
        $receiptLength = 0
        while ($receiptLength -lt $receiptBytes.Length) {
            $count = $receiptStream.Read($receiptBytes, $receiptLength, $receiptBytes.Length - $receiptLength)
            if ($count -eq 0) { break }
            $receiptLength += $count
        }
        if ($receiptLength -gt 8192) { throw 'Update receipt exceeds its byte bound.' }
        $receipt = ([Text.UTF8Encoding]::new($false, $true).GetString($receiptBytes, 0, $receiptLength) | ConvertFrom-Json)
    } finally { $receiptStream.Dispose() }
    if ($receipt.protocol -ne 1 -or $receipt.operation -cnotmatch '^[0-9]+-[0-9]+$' -or $receipt.commit -cnotmatch '^[0-9a-f]{40}$') { throw 'Cannot safely fence an invalid update receipt.' }
    if ($receipt.state -in @('pending','running')) {
        $receipt.state = 'failed'
        $receipt.reason = 'installer-superseded'
        $fencedBytes = [Text.UTF8Encoding]::new($false).GetBytes(($receipt | ConvertTo-Json -Compress))
        if ($fencedBytes.Length -gt 8192) { throw 'Fenced update receipt exceeds its byte bound.' }
        $fencedPath = $updateStatus + '.installer.tmp'
        [IO.File]::WriteAllBytes($fencedPath, $fencedBytes)
        [IO.File]::Replace($fencedPath, $updateStatus, [System.Management.Automation.Language.NullString]::Value)
    }
}

function Invoke-SigyGit {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$GitArgs)
    $useGh = $false
    if (Get-Command gh -ErrorAction SilentlyContinue) {
        $previous = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        & gh auth status *> $null
        if ($LASTEXITCODE -eq 0) {
            $useGh = $true
        }
        $ErrorActionPreference = $previous
    }
    if ($useGh) {
        & git @gitFlags -c credential.helper= -c "credential.helper=!gh auth git-credential" @GitArgs
    } else {
        & git @gitFlags -c credential.helper= @GitArgs
    }
    if ($LASTEXITCODE -ne 0) {
        throw "git failed with exit code $LASTEXITCODE"
    }
}

function Assert-SigyManagedSource {
    param([string]$SourceRoot)
    $origin = & git -C $SourceRoot @gitFlags config --local --get remote.origin.url 2>$null
    if ($LASTEXITCODE -ne 0 -or [string]$origin -ne $repo) {
        throw "Managed sigy source origin is not $repo."
    }
    $dirty = & git -C $SourceRoot @gitFlags status --porcelain=v1 --untracked-files=all 2>$null
    if ($LASTEXITCODE -ne 0 -or $dirty) {
        throw "Managed sigy source has local changes or cannot be inspected. Preserve it and use a clean source directory."
    }
}
$root = $null
$fromGitHub = $false

if ($env:SIGY_SRC) {
    $root = $env:SIGY_SRC
    $fromGitHub = $true
} elseif ($PSScriptRoot) {
    $candidate = Split-Path -Parent $PSScriptRoot
    if (Test-Path -LiteralPath (Join-Path $candidate "crates\sigy\Cargo.toml")) {
        $root = $candidate
    }
}

if (-not $root) {
    $root = Join-Path $env:USERPROFILE ".sigy\src"
    $fromGitHub = $true
}

$root = [System.IO.Path]::GetFullPath([System.IO.Path]::Combine((Get-Location).ProviderPath, $root))

if ($fromGitHub) {
    $git = Get-Command git -ErrorAction SilentlyContinue
    if (-not $git) {
        throw "git is missing. Install Git to fetch $repo."
    }
    $parent = [System.IO.Path]::GetDirectoryName($root)
    if ($parent) {
        [void][System.IO.Directory]::CreateDirectory($parent)
    }
    if (-not (Test-Path -LiteralPath (Join-Path $root ".git"))) {
        if (Test-Path -LiteralPath $root) {
            throw "$root exists and is not a sigy checkout."
        }
        Invoke-SigyGit clone --depth 1 --branch main $repo $root
    }
    Assert-SigyManagedSource $root
    Invoke-SigyGit -C $root fetch --depth 1 $repo main
    Invoke-SigyGit -C $root checkout --detach FETCH_HEAD
    Assert-SigyManagedSource $root
}

Set-Location -LiteralPath $root

$cargoRoot = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE ".cargo" }
$cargoHome = Join-Path $cargoRoot "bin"
if (Test-Path -LiteralPath (Join-Path $cargoHome "cargo.exe")) {
    $env:Path = "$cargoHome;$env:Path"
}

$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargo) {
    Write-Host "Installing Rust 1.98.1 for the current user."
    $installer = Join-Path $env:TEMP "rustup-init.exe"
    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $installer
    & $installer -y --default-toolchain 1.98.1 --profile minimal
    if ($LASTEXITCODE -ne 0) {
        throw "rustup-init failed with exit code $LASTEXITCODE"
    }
    $env:Path = "$cargoHome;$env:Path"
}

$commit = ""
Remove-Item Env:SIGY_GIT_COMMIT -ErrorAction SilentlyContinue
$resolvedCommit = & git -C $root @gitFlags rev-parse HEAD 2>$null
if ($LASTEXITCODE -eq 0 -and $resolvedCommit) {
    $localChanges = & git -C $root @gitFlags status --porcelain=v1 --untracked-files=all 2>$null
    if ($LASTEXITCODE -eq 0 -and -not $localChanges) {
        $commit = ([string]$resolvedCommit).Trim()
        $env:SIGY_GIT_COMMIT = $commit
    }
}

$staging = Join-Path $metadata "stage-$([Guid]::NewGuid().ToString('N'))"
[void][System.IO.Directory]::CreateDirectory($staging)
try {
    if ($commit) {
        $stagingArchive = Join-Path $staging "archive.tar"
        & git -C $root @gitFlags archive --format=tar --output=$stagingArchive $commit 2>$null
        $extracted = $false
        if ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $stagingArchive)) {
            & tar.exe -xf $stagingArchive -C $staging 2>$null
            if ($LASTEXITCODE -eq 0) {
                $extracted = $true
            }
            Remove-Item -LiteralPath $stagingArchive -Force -ErrorAction SilentlyContinue
        }
        if (-not $extracted) {
            Copy-Item -Path (Join-Path $root "*") -Destination $staging -Recurse -Force
        }
    } else {
        Copy-Item -Path (Join-Path $root "*") -Destination $staging -Recurse -Force
    }
    Set-Location -LiteralPath $staging
    & cargo install --path crates/sigy --locked --force
    if ($LASTEXITCODE -ne 0) {
        throw "cargo install failed with exit code $LASTEXITCODE"
    }
} finally {
    Set-Location -LiteralPath $root
    if (Test-Path -LiteralPath $staging) {
        Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
    }
}

if ($commit) {
    $afterCommit = & git -C $root @gitFlags rev-parse HEAD 2>$null
    if ($LASTEXITCODE -ne 0 -or [string]$afterCommit -cne $commit) {
        throw "Source commit changed during installation; installation identity is unproven."
    }
    $afterChanges = & git -C $root @gitFlags status --porcelain=v1 --untracked-files=all 2>$null
    if ($LASTEXITCODE -ne 0 -or $afterChanges) {
        throw "Source changed during installation; installation identity is unproven."
    }
}

if ($commit) {
    $meta = Join-Path $env:USERPROFILE ".sigy"
    [void][System.IO.Directory]::CreateDirectory($meta)
    Set-Content -LiteralPath (Join-Path $meta "installed-commit") -Value $commit
    Write-Host "Installed sigy at $commit."
} else {
    $meta = Join-Path $env:USERPROFILE ".sigy"
    [void][System.IO.Directory]::CreateDirectory($meta)
    Set-Content -LiteralPath (Join-Path $meta "installed-commit") -Value ""
    Write-Host "Installed sigy without a clean commit identity."
}
Write-Host "Check later with: sigy update --check"
Write-Host "Install a newer main commit with: sigy update"
Write-Host "Start with: sigy init --radio"
Write-Host "Open the explorer with: sigy tui"
$ffmpeg = Get-Command ffmpeg -ErrorAction SilentlyContinue
if (-not $ffmpeg) {
    Write-Host "Recording and playback need a trusted FFmpeg. This script does not download it."
}

} finally {
    if ($installLock) { $installLock.Dispose() }
    foreach ($name in $previousEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], 'Process')
    }
    Set-Location -LiteralPath $previousLocation.ProviderPath
}
}
