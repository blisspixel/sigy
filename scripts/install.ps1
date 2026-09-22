# Install the sigy binary from this checkout. This is not cargo verify.
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
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

& cargo install --path crates/sigy --locked --force
if ($LASTEXITCODE -ne 0) {
    throw "cargo install failed with exit code $LASTEXITCODE"
}

Write-Host "Installed sigy. Next: sigy --help"
Write-Host "Create a private library with: sigy --data-dir PATH_TO_LIBRARY library init"
$ffmpeg = Get-Command ffmpeg -ErrorAction SilentlyContinue
if (-not $ffmpeg) {
    Write-Host "Recording and playback need a trusted FFmpeg. This script does not download it."
}
