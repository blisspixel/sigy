param([Parameter(Mandatory=$true)][string]$Config)
$ErrorActionPreference = 'Stop'
if (Get-Variable PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) { $PSNativeCommandUseErrorActionPreference = $false }
$utf8 = [Text.UTF8Encoding]::new($false, $true)
function Read-BoundedJson([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $bytes = New-Object byte[] 8193
        $total = 0
        while ($total -lt $bytes.Length) {
            $count = $stream.Read($bytes, $total, $bytes.Length - $total)
            if ($count -eq 0) { break }
            $total += $count
        }
        if ($total -gt 8192) { throw 'update JSON bound' }
        return ($utf8.GetString($bytes, 0, $total) | ConvertFrom-Json)
    } finally { $stream.Dispose() }
}
$scope = Read-BoundedJson $Config
if ($scope.protocol -ne 1 -or $scope.commit -cnotmatch '^[0-9a-f]{40}$' -or $scope.operation -cnotmatch '^[0-9]+-[0-9]+$') { throw 'update scope invalid' }
$lock = $null
$reason = 'helper-startup-failed'
$published = $false

function Write-Outcome([string]$State, [string]$Reason) {
    $receipt = [ordered]@{ protocol=1; operation=$scope.operation; state=$State; commit=$scope.commit; install_root=$scope.install_root; reason=$Reason }
    $json = $receipt | ConvertTo-Json -Compress
    if ($utf8.GetByteCount($json) -gt 8192) { throw 'update receipt bound' }
    $temporaryReceipt = $scope.status + '.tmp-' + $scope.operation
    [IO.File]::WriteAllText($temporaryReceipt, $json, $utf8)
    if (Test-Path -LiteralPath $scope.status) { [IO.File]::Replace($temporaryReceipt, $scope.status, [System.Management.Automation.Language.NullString]::Value) }
    else { [IO.File]::Move($temporaryReceipt, $scope.status) }
}
function Assert-Source {
    $env:GIT_TERMINAL_PROMPT = '0'
    $env:GIT_CONFIG_NOSYSTEM = '1'
    $env:GIT_CONFIG_GLOBAL = 'NUL'
    $gitOpts = @('-c', 'core.abbrev=40', '-c', 'core.fsmonitor=', '-c', 'core.hooksPath=NUL', '-c', 'http.followRedirects=false')
    $head = & git -C $scope.source @gitOpts rev-parse HEAD 2>$null
    if ($LASTEXITCODE -ne 0 -or [string]$head -cne $scope.commit) { throw 'source-commit-changed' }
    $origin = & git -C $scope.source @gitOpts config --local --get remote.origin.url 2>$null
    if ($LASTEXITCODE -ne 0 -or [string]$origin -cne 'https://github.com/blisspixel/sigy.git') { throw 'source-origin-changed' }
    $dirty = & git -C $scope.source @gitOpts status --porcelain=v1 --untracked-files=all 2>$null
    if ($LASTEXITCODE -ne 0 -or $dirty) { throw 'source-not-clean' }
}
function Get-BinaryHash([string]$Path) {
    $algorithm = [Security.Cryptography.SHA256]::Create()
    $stream = $null
    try {
        $stream = [IO.File]::OpenRead($Path)
        return [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '')
    } finally {
        if ($stream) { $stream.Dispose() }
        $algorithm.Dispose()
    }
}
try {
    $reason = 'parent-exit-timeout'
    $parent = Get-Process -Id $scope.parent_pid -ErrorAction SilentlyContinue
    if ($parent -and -not $parent.WaitForExit(60000)) { throw $reason }
    $reason = 'install-lock-unavailable'
    $until = [DateTime]::UtcNow.AddSeconds(30)
    do {
        try { $lock = [IO.File]::Open($scope.lock, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
        catch { if ([DateTime]::UtcNow -ge $until) { throw $reason }; Start-Sleep -Milliseconds 100 }
    } while (-not $lock)
    $current = Read-BoundedJson $scope.status
    if ($current.operation -cne $scope.operation -or $current.state -cne 'pending') { throw 'update-superseded' }
    Write-Outcome 'running' $null
    $env:GIT_TERMINAL_PROMPT = '0'
    $env:CARGO_HOME = $scope.cargo_home
    if (Test-Path -LiteralPath (Join-Path $scope.cargo_bin 'cargo.exe')) { $env:Path = "$($scope.cargo_bin);$env:Path" }
    $reason = 'source-validation-failed'
    Assert-Source
    $env:SIGY_GIT_COMMIT = $scope.commit
    Set-Location -LiteralPath $scope.source
    $reason = 'cargo-build-failed'
    & cargo install --path crates/sigy --locked --force --root $scope.stage *> $null
    if ($LASTEXITCODE -ne 0) { throw $reason }
    $reason = 'source-changed-during-build'
    Assert-Source
    $reason = 'staged-binary-validation-failed'
    $binary = Join-Path $scope.stage 'bin/sigy.exe'
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'staged-binary-missing' }
    $expectedHash = Get-BinaryHash $binary
    $reason = 'binary-publication-failed'
    $bin = Join-Path $scope.install_root 'bin'
    [void][IO.Directory]::CreateDirectory($bin)
    $target = Join-Path $bin 'sigy.exe'
    $temporary = Join-Path $bin ('.sigy-update-' + $scope.operation + '.exe')
    $backup = Join-Path $bin ('.sigy-previous-' + $scope.operation + '.exe')
    [IO.File]::Copy($binary, $temporary, $false)
    if (Test-Path -LiteralPath $target) { [IO.File]::Replace($temporary, $target, $backup) }
    else { [IO.File]::Move($temporary, $target) }
    $published = $true
    if ((Get-BinaryHash $target) -cne $expectedHash) { throw 'published-binary-mismatch' }
    $reason = 'published-receipt-failed'
    [IO.File]::WriteAllText($scope.marker, "$($scope.commit)`n", $utf8)
    Write-Outcome 'succeeded' $null
    if (Test-Path -LiteralPath $backup) { try { [IO.File]::Delete($backup) } catch { } }
} catch {
    if ($_.Exception.Message -eq 'update-superseded') { exit 1 }
    if ($reason -eq 'source-validation-failed' -and $_.Exception.Message -in @('source-commit-changed','source-origin-changed','source-not-clean')) { $reason = $_.Exception.Message }
    if ($published -and $reason -eq 'binary-publication-failed') { $reason = 'published-receipt-failed' }
    # A timed-out or superseded helper never owns another operation's outcome.
    if ($lock) {
        try {
            $current = Read-BoundedJson $scope.status
            if ($current.operation -ceq $scope.operation) { Write-Outcome 'failed' $reason }
        } catch { }
    }
    exit 1
} finally {
    if ($lock) { $lock.Dispose() }
}
