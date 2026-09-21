param([string]$Decoder)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $Decoder) { $Decoder = (Get-Command ffmpeg -CommandType Application -ErrorAction Stop).Source }
$Decoder = (Resolve-Path -LiteralPath $Decoder).Path
$oldDecoder = $env:SIGY_TEST_FFMPEG
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    $env:SIGY_TEST_FFMPEG = $Decoder
    & cargo test --locked -p sigy --test service recording_fixtures -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Native media verification failed.' }
} finally {
    $env:SIGY_TEST_FFMPEG = $oldDecoder
    Pop-Location
}
