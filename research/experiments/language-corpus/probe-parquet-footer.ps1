param([int]$MaxFiles = 16)
$ErrorActionPreference = 'Stop'
$planRoot = $PSScriptRoot
$probeLimit = 1048576L
$convertRevision = '168de341b3db6859a9bac1c50a2ef5e3b47647e0'
$receiptPath = Join-Path $planRoot 'range-probe-receipts.json'
$receipts = [Collections.Generic.List[object]]::new()
if (Test-Path -LiteralPath $receiptPath) {
    foreach ($prior in @(Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json)) { $receipts.Add($prior) }
}
$script:receivedBytes = [long](($receipts | Measure-Object response_body_bytes_read -Sum).Sum)
$handler = [Net.Http.HttpClientHandler]::new()
$handler.AllowAutoRedirect = $false
$handler.AutomaticDecompression = [Net.DecompressionMethods]::None
$client = [Net.Http.HttpClient]::new($handler)
$client.Timeout = [TimeSpan]::FromSeconds(45)
function Read-StrictRange([string]$url, [long]$start, [int]$length, [long]$size, [string]$destination) {
    if ($script:receivedBytes + $length -gt $probeLimit) { throw 'Cumulative probe allowance exhausted' }
    $end = $start + $length - 1
    $uri = [uri]$url
    for ($hop = 0; $hop -le 4; $hop++) {
        if ($uri.Scheme -ne 'https') { throw 'Non-HTTPS redirect refused' }
        $req = [Net.Http.HttpRequestMessage]::new([Net.Http.HttpMethod]::Get, $uri)
        $req.Headers.Range = [Net.Http.Headers.RangeHeaderValue]::new($start, $end)
        $req.Headers.AcceptEncoding.ParseAdd('identity')
        $res = $null
        $receipt = [ordered]@{
            request_url_without_query = $uri.GetLeftPart([UriPartial]::Path)
            request_start = $start; request_end = $end; source_size = $size
            status = $null; content_length = $null; content_range = $null
            response_body_bytes_read = 0; result = 'started'
        }
        try {
            $res = $client.SendAsync($req, [Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
            $receipt.status = [int]$res.StatusCode
            $receipt.content_length = $res.Content.Headers.ContentLength
            $receipt.content_range = [string]$res.Content.Headers.ContentRange
            if ($receipt.status -in @(301,302,303,307,308)) {
                if ($null -eq $res.Headers.Location) { throw 'Redirect without destination' }
                $uri = [uri]::new($uri, $res.Headers.Location)
                $receipt.result = 'redirect_headers_only'
                continue
            }
            if ($receipt.status -ne 206) { throw "Strict range rejected: status $($receipt.status)" }
            if ($res.Content.Headers.ContentLength -ne $length -or [string]$res.Content.Headers.ContentRange -ne "bytes $start-$end/$size" -or $res.Content.Headers.ContentEncoding.Count -ne 0) { throw 'Unexpected range headers' }
            $buffer = [byte[]]::new($length)
            $stream = $res.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
            try {
                $offset = 0
                while ($offset -lt $length) {
                    $read = $stream.Read($buffer, $offset, $length - $offset)
                    if ($read -eq 0) { throw 'Truncated range body' }
                    $offset += $read
                    $script:receivedBytes += $read
                    $receipt.response_body_bytes_read += $read
                }
            } finally { $stream.Dispose() }
            [IO.File]::WriteAllBytes($destination, $buffer)
            $receipt.result = 'verified_partial_response'
            $receipt.body_sha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($buffer)).ToLowerInvariant()
            return ,$buffer
        } catch {
            $receipt.result = 'failed: ' + $_.Exception.Message
            throw
        } finally {
            $receipts.Add([pscustomobject]$receipt)
            $receipts | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $receiptPath -Encoding utf8NoBOM
            if ($null -ne $res) { $res.Dispose() }
            $req.Dispose()
        }
    }
    throw 'Redirect limit exceeded'
}
try {
    $inventory = Get-Content -LiteralPath (Join-Path $planRoot 'parquet-inventory.json') -Raw | ConvertFrom-Json
    foreach ($file in @($inventory.files | Sort-Object config,split | Select-Object -First $MaxFiles)) {
        $base = "$($file.config)-$($file.split)"
        $footerPath = Join-Path $planRoot "$base.footer.bin"
        if (Test-Path -LiteralPath $footerPath) { continue }
        $url = "https://huggingface.co/datasets/google/fleurs/resolve/$convertRevision/$($file.config)/$($file.split)/0000.parquet"
        $tail = Read-StrictRange $url ($file.size - 8) 8 $file.size (Join-Path $planRoot "$base.tail.bin")
        if ([Text.Encoding]::ASCII.GetString($tail,4,4) -ne 'PAR1') { throw 'Unexpected Parquet trailer' }
        $footerLength = [BitConverter]::ToUInt32($tail,0)
        if ($footerLength -gt 262144 -or $footerLength -eq 0) { throw "Footer length outside bounded parser plan: $footerLength" }
        $null = Read-StrictRange $url ($file.size - 8 - $footerLength) $footerLength $file.size $footerPath
        [pscustomobject]@{config=$file.config;split=$file.split;footer_bytes=$footerLength;cumulative_probe_bytes=$script:receivedBytes} | ConvertTo-Json -Compress
    }
} finally { $client.Dispose(); $handler.Dispose() }
