# Bounded Parquet acquisition review

Reviewed: 2026-09-22. Result: strict range access is available, but the reviewed row-group route does **not** fit the 10 GiB cumulative download allowance for the frozen 112 clips. No full Parquet file, audio clip, archive or model was downloaded. No inference was performed. The derived selection and metadata are retained here.

## Immutable source identity

The [Hub refs endpoint](https://huggingface.co/api/datasets/google/fleurs/refs) resolved `refs/convert/parquet` to immutable commit `168de341b3db6859a9bac1c50a2ef5e3b47647e0`. The source remains `70bb2e84b976b7e960aa89f1c648e09c59f894dd`. Do not use the mutable convert ref for subsequent acquisition.

All sixteen selected train/test files have equal declared sizes and LFS SHA-256 identities in both commits. The converted path `{config}/{split}/0000.parquet` refers to the same declared blob as source path `parquet-data/{config}/{split}-00000-of-00001.parquet`. Exact metadata URLs, paths and hashes are in `parquet-source-linkage.json`. This checks publisher metadata identity, not a locally verified whole-file hash. Example immutable resolve URL:

`https://huggingface.co/datasets/google/fleurs/resolve/168de341b3db6859a9bac1c50a2ef5e3b47647e0/fr_fr/train/0000.parquet`

The Viewer inventory is complete for these files: `partial=false`, no pending or failed entries, one file per selected subset/split. Full files total 20,450,787,987 bytes. [Viewer Parquet documentation](https://huggingface.co/docs/dataset-viewer/parquet) explains the convert branch and original-file linkage for Parquet-native datasets.

## Strict range probe and byte receipt

`probe-parquet-footer.ps1` used HTTP headers-first reads with automatic redirects and decompression disabled. Every request included an exact byte range and `Accept-Encoding: identity`. It rejected status other than 206, incorrect `Content-Range`, a mismatched `Content-Length`, or a content encoding before reading the file body. Redirects were followed explicitly with the range preserved; their bodies were not read. The cumulative application-body allowance was 1,048,576 bytes, including earlier receipts on rerun.

The probe read the last eight bytes of each file, checked `PAR1`, and requested only its declared Thrift footer. All sixteen files passed. There were 32 verified HTTP 206 responses and 32 HTTP 302 redirect responses, through `huggingface.co` and `us.aws.cdn.hf.co`. No status/header/truncation failures occurred in this probe. `range-probe-receipts.json` retains each requested interval, exact response range/length, body count and SHA-256. Signed redirect query strings were not persisted.

| Application response bodies | Exact bytes |
| --- | ---: |
| Sixteen eight-byte trailers | 128 |
| Sixteen Parquet metadata footers | 159,657 |
| Total bounded file probe | **159,785** |
| Hub/Viewer JSON metadata fetched by local commands during this investigation | 100,989 |
| Total recorded local-command response content | **260,774** |

The metadata count includes 84,925 bytes for the inventory, 349 for refs, 13,027 for all source/convert linkage requests, 1,746 for exploratory source/convert/streaming tree requests, and 942 for the streaming audio-directory inventory. These are decoded UTF-8 metadata content counts. Probe counts are exact application body bytes read. They do not measure TLS, HTTP headers, network retransmission, socket read-ahead or the separate documentation browser's traffic; they are not a wire-byte measurement. No authentication was used.

## What the footer proves

`inspect-parquet-footer.mjs` is a dependency-free, bounded offline Compact Protocol inspector for these research footers, following the [official Parquet Thrift schema](https://github.com/apache/parquet-format/blob/master/src/main/thrift/parquet.thrift). It is not product code or a production Parquet reader. It consumes complete footers, checks lengths and row counts, and emits only structural metadata. Its outputs are in `parquet-footer-metadata.json`; full row counts match the pinned TSV counts.

There are 35 row groups across the sixteen files. Each audio byte column uses Snappy and lacks both offset and column indexes. The selected filenames each fall within one row group's published filename bounds. Those bounds identify candidate row groups, not individual row offsets or an independently decoded filename match. The row-group estimate conservatively includes each such audio column once; exact ranges and selected filenames are in `row-group-acquisition-estimate.json`.

The needed audio columns sum to **17,200,600,678 bytes, about 16.02 GiB**, before reference/ID columns, headers, retained metadata, models or retries. Consequently, row-group projection is not a proven route under the current allowance. Predicate filtering does not fix this: the audio payload is compressed in large dictionaries, with the dictionary-page offset near the start and the data-page offset near the end of each audio chunk.

For example, French train group 0 has 1,000 values. Its audio dictionary begins at byte 33,134 and its data page begins at 730,803,574. The audio column is 730,771,729 bytes compressed. Reading a tiny row-ID page cannot supply one embedded WAV without the corresponding dictionary data. [Parquet file layout](https://parquet.apache.org/docs/file-format/) and the [page-index specification](https://parquet.apache.org/docs/file-format/pageindex/) describe the relevant boundaries. No page or audio dictionary payload was fetched in this probe.

This does not prove that every bespoke partial Snappy decoder or other copy of FLEURS is unusable. It establishes that the reviewed immutable files, normal dictionary decoding and row-group ranges do not give the required small bounded route. An unmeasured custom decoder is not evidence for a lower transfer reservation.

## Remaining gate and alternatives

Keep the frozen manifest unchanged and acquisition blocked. A later route must establish exact row identity, license/provenance, response-size caps, checksums and a cumulative reservation before any media transfer. A maintained source exposing individual files or genuinely small indexed pages would be suitable for review. Re-freezing the corpus around another licensed source is a separate explicit selection change, not an automatic fallback.

The repository's `streaming` branch was also checked at immutable commit `fe329515c501b7c6c659cb209658044da1c1a831`. Its French audio directory still lists `dev.tar.gz`, `test.tar.gz` and `train.tar.gz`, with the same archive sizes. The branch name alone does not provide individual-clip range access; `streaming-branch-metadata.json` preserves this bounded check.

Do not download all dictionaries, switch to the 15.01 GiB source archive set, relax speaker separation, substitute new clips, or treat the approximately 82 MB retained-audio estimate as the required transfer budget. The evidence now supports a precise negative result for this acquisition route rather than a silent allowance increase.
