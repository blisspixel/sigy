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
$root = $null
$fromGitHub = $false

if ($env:SIGY_SRC) {
    $root = $env:SIGY_SRC
    $fromGitHub = $true
} elseif ($PSScriptRoot) {
    $candidate = Split-Path -Parent $PSScriptRoot
    if (Test-Path (Join-Path $candidate "crates\sigy\Cargo.toml")) {
        $root = $candidate
    }
}

if (-not $root) {
    $root = Join-Path $env:USERPROFILE ".sigy\src"
    $fromGitHub = $true
}

if ($fromGitHub) {
    $git = Get-Command git -ErrorAction SilentlyContinue
    if (-not $git) {
        throw "git is missing. Install Git and authenticate to $repo."
    }
    $parent = Split-Path -Parent $root
    if ($parent) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    if (-not (Test-Path (Join-Path $root ".git"))) {
        if (Test-Path $root) {
            throw "$root exists and is not a sigy checkout."
        }
        Invoke-SigyGit clone --depth 1 --branch main $repo $root
    }
    Invoke-SigyGit -C $root -c core.abbrev=40 fetch --depth 1 origin main
    & git -C $root checkout --detach FETCH_HEAD
    if ($LASTEXITCODE -ne 0) {
        throw "git checkout failed with exit code $LASTEXITCODE"
    }
}

Set-Location $root

$cargoHome = Join-Path $env:USERPROFILE ".cargo\bin"
if (Test-Path (Join-Path $cargoHome "cargo.exe")) {
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
& git -C $root -c core.abbrev=40 rev-parse HEAD 2>$null | Out-Null
if ($LASTEXITCODE -eq 0) {
    $commit = (& git -C $root -c core.abbrev=40 rev-parse HEAD).Trim()
    $env:SIGY_GIT_COMMIT = $commit
}

& cargo install --path crates/sigy --locked --force
if ($LASTEXITCODE -ne 0) {
    throw "cargo install failed with exit code $LASTEXITCODE"
}

if ($commit) {
    $meta = Join-Path $env:USERPROFILE ".sigy"
    New-Item -ItemType Directory -Force -Path $meta | Out-Null
    Set-Content -LiteralPath (Join-Path $meta "installed-commit") -Value $commit
    Write-Host "Installed sigy at $commit."
} else {
    Write-Host "Installed sigy."
}
Write-Host "Check later with: sigy update --check"
Write-Host "Install a newer main commit with: sigy update"
Write-Host "Create a private library with: sigy --data-dir PATH_TO_LIBRARY library init"
$ffmpeg = Get-Command ffmpeg -ErrorAction SilentlyContinue
if (-not $ffmpeg) {
    Write-Host "Recording and playback need a trusted FFmpeg. This script does not download it."
}
