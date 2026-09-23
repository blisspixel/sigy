# Install sigy from https://github.com/blisspixel/sigy. This is not cargo verify.
$ErrorActionPreference = "Stop"
if (Get-Variable PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}
$env:GIT_TERMINAL_PROMPT = "0"

$repo = "https://github.com/blisspixel/sigy.git"

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
        & git -c credential.helper= -c "credential.helper=!gh auth git-credential" @GitArgs
    } else {
        & git @GitArgs
    }
    if ($LASTEXITCODE -ne 0) {
        throw "git failed with exit code $LASTEXITCODE"
    }
}

function Assert-SigyManagedSource {
    param([string]$SourceRoot)
    $origin = & git -C $SourceRoot config --local --get remote.origin.url
    if ($LASTEXITCODE -ne 0 -or [string]$origin -ne $repo) {
        throw "Managed sigy source origin is not $repo."
    }
    $dirty = & git -C $SourceRoot status --porcelain=v1 --untracked-files=all
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
    Invoke-SigyGit -C $root -c core.abbrev=40 fetch --depth 1 $repo main
    & git -C $root checkout --detach FETCH_HEAD
    if ($LASTEXITCODE -ne 0) {
        throw "git checkout failed with exit code $LASTEXITCODE"
    }
    Assert-SigyManagedSource $root
}

Set-Location -LiteralPath $root

$cargoHome = Join-Path $env:USERPROFILE ".cargo\bin"
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
$resolvedCommit = & git -C $root -c core.abbrev=40 rev-parse HEAD 2>$null
if ($LASTEXITCODE -eq 0 -and $resolvedCommit) {
    $localChanges = & git -C $root status --porcelain=v1 --untracked-files=all
    if ($LASTEXITCODE -eq 0 -and -not $localChanges) {
        $commit = ([string]$resolvedCommit).Trim()
        $env:SIGY_GIT_COMMIT = $commit
    }
}

& cargo install --path crates/sigy --locked --force
if ($LASTEXITCODE -ne 0) {
    throw "cargo install failed with exit code $LASTEXITCODE"
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
Write-Host "Create a private library with: sigy --data-dir PATH_TO_LIBRARY library init"
$ffmpeg = Get-Command ffmpeg -ErrorAction SilentlyContinue
if (-not $ffmpeg) {
    Write-Host "Recording and playback need a trusted FFmpeg. This script does not download it."
}
