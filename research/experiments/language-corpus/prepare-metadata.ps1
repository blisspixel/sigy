$ErrorActionPreference = 'Stop'
$planRoot = $PSScriptRoot
$revision = '70bb2e84b976b7e960aa89f1c648e09c59f894dd'
$configs = @('fr_fr','es_419','pt_br','ar_eg','sw_ke','hi_in','cmn_hans_cn','en_us')
function Get-Hash([string]$text) {
    [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($text))).ToLowerInvariant()
}
$tables = @{}
$sources = [Collections.Generic.List[object]]::new()
foreach ($config in $configs) {
    foreach ($split in @('train','test')) {
        $url = "https://huggingface.co/datasets/google/fleurs/raw/$revision/data/$config/$split.tsv"
        $response = Invoke-WebRequest -Uri $url -TimeoutSec 60
        $tsv = [string]$response.Content
        $lines = @($tsv -split "`n" | Where-Object { $_.TrimEnd("`r").Length -gt 0 })
        $rows = for ($index = 0; $index -lt $lines.Count; $index++) {
            $fields = $lines[$index].TrimEnd("`r") -split "`t"
            if ($fields.Count -ne 7) { throw "Unexpected TSV columns for $config/$split at $index" }
            [pscustomobject]@{
                Config = $config; Split = $split; TsvRowIndex = $index
                SentenceId = [int]$fields[0]; Filename = $fields[1]
                RawReference = $fields[2]; NormalizedReference = $fields[3]
                NumSamples = [int]$fields[5]; Gender = $fields[6]
            }
        }
        $tables["$config/$split"] = @($rows)
        $sources.Add([pscustomobject]@{
            config = $config; split = $split; url = $url
            metadata_utf8_bytes = [Text.Encoding]::UTF8.GetByteCount($tsv)
            metadata_text_sha256 = Get-Hash $tsv; row_count = $rows.Count
            unique_sentence_ids = @($rows.SentenceId | Sort-Object -Unique).Count
        })
    }
}
$partitions = @{}
foreach ($split in @('train','test')) {
    $common = @($tables["en_us/$split"] | Where-Object { $_.NumSamples -ge 64000 -and $_.NumSamples -le 320000 } | Select-Object -ExpandProperty SentenceId -Unique)
    foreach ($config in $configs) {
        $eligibleIds = @($tables["$config/$split"] | Where-Object { $_.NumSamples -ge 64000 -and $_.NumSamples -le 320000 } | Select-Object -ExpandProperty SentenceId -Unique)
        $common = @($common | Where-Object { $_ -in $eligibleIds })
    }
    $count = if ($split -eq 'train') { 4 } else { 10 }
    $ranked = @($common | ForEach-Object { [pscustomobject]@{Id=$_;Rank=(Get-Hash "sigy-fleurs-screen-v1|$split|$_")} } | Sort-Object Rank,Id)
    if ($ranked.Count -lt $count) { throw "Insufficient common eligible groups: $split" }
    $partitions[$split] = @($ranked | Select-Object -First $count -ExpandProperty Id)
}
if (@($partitions.train | Where-Object { $_ -in $partitions.test }).Count -ne 0) { throw 'Cross-partition sentence leakage' }
$selection = foreach ($config in $configs) {
    foreach ($split in @('train','test')) {
        $table = $tables["$config/$split"]
        foreach ($id in $partitions[$split]) {
            $eligible = @($table | Where-Object { $_.SentenceId -eq $id -and $_.NumSamples -ge 64000 -and $_.NumSamples -le 320000 })
            $chosen = $eligible | Sort-Object @{Expression={Get-Hash "sigy-fleurs-screen-v1|$config|$split|$($_.Filename)"}},Filename | Select-Object -First 1
            $enRef = @($tables["en_us/$split"] | Where-Object { $_.SentenceId -eq $id } | Select-Object -ExpandProperty RawReference -Unique)
            if ($enRef.Count -ne 1) { throw "Ambiguous English reference for $split/$id" }
            [pscustomobject]@{
                asset_id = "$config/$split/$($chosen.Filename)"
                partition = if($split -eq 'train'){'calibration'}else{'holdout'}
                config = $config; split = $split; sentence_group_id = $id
                original_filename = $chosen.Filename
                source_tsv_row_index_zero_based = $chosen.TsvRowIndex
                viewer_row_index = $null
                viewer_row_index_verified = $false
                audio_download_bytes = $null
                num_samples = $chosen.NumSamples; sample_rate_hz = 16000
                duration_seconds = $chosen.NumSamples / 16000
                source_gender_label = $chosen.Gender
                raw_transcription_sha256 = Get-Hash $chosen.RawReference
                normalized_transcription_sha256 = Get-Hash $chosen.NormalizedReference
                parallel_english_reference_sha256 = Get-Hash $enRef[0]
                decoded_pcm16_wav_estimate_bytes = 44 + (2 * $chosen.NumSamples)
                float32_wav_estimate_bytes = 44 + (4 * $chosen.NumSamples)
                audio_sha256 = $null; download_status = 'not_downloaded'
            }
        }
    }
}
$fullOverlap = foreach ($config in $configs) {
    $trainIds = @($tables["$config/train"].SentenceId | Sort-Object -Unique)
    $testIds = @($tables["$config/test"].SentenceId | Sort-Object -Unique)
    [pscustomobject]@{config=$config; common_train_test_sentence_ids=@($trainIds | Where-Object { $_ -in $testIds }).Count}
}
$calibrationIds = @($selection | Where-Object partition -eq 'calibration' | Select-Object -ExpandProperty sentence_group_id -Unique)
$holdoutIds = @($selection | Where-Object partition -eq 'holdout' | Select-Object -ExpandProperty sentence_group_id -Unique)
$globalOverlap = @($calibrationIds | Where-Object { $_ -in $holdoutIds })
if ($globalOverlap.Count -ne 0) { throw 'Global cross-language partition leakage' }
$allTrainIds = @($configs | ForEach-Object { $tables["$_/train"].SentenceId } | Sort-Object -Unique)
$allTestIds = @($configs | ForEach-Object { $tables["$_/test"].SentenceId } | Sort-Object -Unique)
$allOverlap = @($allTrainIds | Where-Object { $_ -in $allTestIds })
if ($allOverlap.Count -ne 0) { throw 'Global source train/test sentence overlap' }
[pscustomobject]@{
    schema_version = 1; status = 'metadata_plan_only'; reviewed = '2026-09-22'
    dataset = 'google/fleurs'; revision = $revision; license = 'CC-BY-4.0'
    access = @{public=$true; gated=$false; authentication_required=$false; audio_download_authorized=$false; remote_upload_authorized=$false}
    purpose = 'ASR and parallel-reference translation smoke screening only; not statistical, speaker, regional or broadcast qualification'
    acquisition_status = 'blocked_pending_bounded_per_asset_route; full_archives_exceed_10_GiB_cumulative_download_allowance'
    reference_policy = 'Hashes and locators only here; references must be extracted into evaluator-only files. Workers get opaque clip ids and audio only. Calibration and holdout runners must have separate input roots.'
    speaker_evidence = 'Dataset card says train speakers differ from dev/test speakers. No claim that dev and test speakers differ, or that selected clips have independently verified speaker IDs.'
    selected_sentence_ids = $partitions; metadata_sources = $sources
    split_overlap_check = $fullOverlap
    global_selected_sentence_id_overlap = $globalOverlap
    global_all_source_train_test_sentence_id_overlap = $allOverlap
    source_train_unique_sentence_ids = $allTrainIds.Count
    source_test_unique_sentence_ids = $allTestIds.Count
    selection_seed = 'sigy-fleurs-screen-v1'; assets = @($selection)
} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $planRoot 'fleurs-screening-manifest.json') -Encoding utf8NoBOM
$selection | Group-Object config | ForEach-Object {
    [pscustomobject]@{config=$_.Name;clips=$_.Count;seconds=($_.Group | Measure-Object duration_seconds -Sum).Sum;pcm16_estimate_bytes=($_.Group | Measure-Object decoded_pcm16_wav_estimate_bytes -Sum).Sum;float32_estimate_bytes=($_.Group | Measure-Object float32_wav_estimate_bytes -Sum).Sum}
} | ConvertTo-Json
