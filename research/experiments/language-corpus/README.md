# Proposed language screening corpus

Reviewed: 2026-09-22. Metadata planning only. No audio, model assets, inference, or paid requests were used. The full archives and [bounded Parquet route](parquet-acquisition-review.md) exceeded the initial 10 GiB allowance, which the user has since raised to 100 GB. The [acquisition route decision](acquisition-route-decision.md) now selects the revision-pinned original archives for their publisher whole-object hashes. A one-clip [Dataset Viewer filter pilot](../../30-language-pipeline-evaluation.md#frozen-fleurs-screening-selection-and-acquisition-gate-2026-09-22) showed individual access, but it did not establish all 112 identities and has weaker immutable provenance. The [alternative-route review](alternative-route-review.md) found a smaller test-only archive path but no complete replacement calibration set. This directory retains the frozen selection and derived metadata; repeat network probes in an ignored working copy so transient files are not committed.

## Exact screening selection

Use `google/fleurs` at revision `70bb2e84b976b7e960aa89f1c648e09c59f894dd`. [Pinned dataset card](https://huggingface.co/datasets/google/fleurs/blob/70bb2e84b976b7e960aa89f1c648e09c59f894dd/README.md), [repository metadata](https://huggingface.co/api/datasets/google/fleurs), [official split metadata](https://datasets-server.huggingface.co/splits?dataset=google%2Ffleurs).

`fleurs-screening-manifest.json` contains 112 exact filename and TSV row selections: four calibration clips from `train` and ten holdout clips from `test` for each of eight subsets. Its `acquisition_status` records the initial cap at freeze time and is not the current authorization. Keep its bytes unchanged because the scorer pins its SHA-256. This is a smoke comparison, not statistical qualification or a sufficient judge-calibration corpus.

| Survey language | Actual subset | Calibration / holdout | Selected duration, seconds |
| --- | --- | --- | --- |
| French | `fr_fr` | 4 / 10 | 155.40 |
| Spanish | `es_419` | 4 / 10 | 158.16 |
| Portuguese | `pt_br` | 4 / 10 | 174.66 |
| Arabic | `ar_eg` | 4 / 10 | 135.60 |
| Swahili | `sw_ke` | 4 / 10 | 203.70 |
| Hindi | `hi_in` | 4 / 10 | 149.04 |
| Mandarin | `cmn_hans_cn` | 4 / 10 | 185.56 |
| English | `en_us` | 4 / 10 | 123.06 |

The total is 1,285.18 seconds, including 1,162.12 non-English seconds, about 90.42%. Corpus locale labels describe this sampling frame, not the capability of a recognizer. `fr_fr` cannot qualify `fr-CA`; an Arabic corpus locale does not qualify every spoken variety. Mandarin's reference script label does not become acoustic script evidence.

Freeze rule, implemented by the metadata-only `prepare-metadata.ps1`:

1. Fetch the sixteen revision-pinned `data/{subset}/{train|test}.tsv` metadata files. Their URLs, exact row counts, UTF-8 text hashes and byte counts are recorded. No audio is fetched.
2. Treat the first TSV field as the parallel sentence group ID, not a unique recording ID. Keep recordings with `64000 <= num_samples <= 320000`, or 4 to 20 seconds at 16 kHz. Intersect eligible sentence IDs across all eight subsets separately for each split.
3. Rank each common ID by lowercase SHA-256 of UTF-8 `sigy-fleurs-screen-v1|{split}|{decimal-id}`. Select the first four train and first ten test IDs. Rank eligible recordings for each selected ID by SHA-256 of `sigy-fleurs-screen-v1|{subset}|{split}|{filename}` and retain the first. Filenames break ties.
4. Preserve the original filename, zero-based source TSV row, sentence ID, sample count, native reference hashes, and matching English parallel-reference hash. No plaintext references or ephemeral signed asset queries are persisted in this plan.

Exact shared calibration IDs: `1087, 264, 773, 831`. Exact shared holdout IDs: `1966, 1793, 1762, 1881, 1803, 1790, 1866, 1939, 1740, 1742`.

## Leakage controls and limits

All selected calibration and holdout sentence groups are disjoint across the eight languages. The metadata check also verifies the union of all sixteen source TSV sentence-ID sets has no train/test overlap, not merely that each language passes independently. Parallel recordings, translations, corruptions, resampling and other derived fixtures must inherit the same partition. The 112 clips represent fourteen shared semantic groups; they are not 112 independent semantic samples.

The dataset card says train speakers differ from dev/test speakers. It does not establish dev/test speaker separation. This plan uses train versus test and preserves that narrower upstream claim; it cannot independently audit speaker identities because those identities are absent from the reviewed row schema. Gender labels are retained as source metadata, not inferred speaker identity or balanced sampling evidence.

The evaluator may reconstruct references from the pinned TSVs after hash verification. Candidate workers must receive only opaque clip IDs and audio, not filenames, sentence IDs, source transcripts, English references, or this manifest. Keep calibration and holdout input roots separate. Freeze thresholds after calibration and before holdout scoring. A paired English reference is a translation comparison aid, not proof that the spoken recording matches its published reference. There are no word timestamps, code-switch boundaries, music, silence, broadcast-noise or live-stream qualifications here. Existing benchmark exposure during model pretraining is unknown and cannot be excluded by this split.

## Access, license and download gate

The Hub repository is public and ungated. Its card declares CC BY 4.0. Preserve supplied dataset creator/citation information, copyright and license notices, source links and modifications with any retained or shared subset; the repository's authorship policy does not remove third-party attribution duties. [License terms](https://creativecommons.org/licenses/by/4.0/). No account or paid service is required for the reviewed metadata. Remote provider upload and provider-specific terms remain a separate review and are not authorized by this manifest.

The [pinned card acquisition](card-provenance-pilot.md) retained and hashed the revision's 385,614-byte card on 2026-09-23. A separate bounded request retained the official CC BY 4.0 legal text. The [pilot attribution packet](fleurs-subset-attribution.md) records the supplied credits, license evidence, and selected identities. Archive-notice review remains a project provenance gate before using acquired audio. The card is provenance evidence, not a selected recording.

The [Arabic calibration reference pilot](arabic-reference-pilot.md) now retains and validates one pinned `ar_eg` train TSV and selected row for evaluator-only use. It contains no audio or recognizer result. Hindi, Spanish and holdout references remain unrequested.

Actual selected audio transfer sizes, encodings and content hashes remain **unknown**. Derived storage estimates from the selected sample counts are 41,130,688 bytes for mono PCM16 WAV with a 44-byte header, or 82,256,448 bytes for float32 with that simplified header. These are mathematical storage estimates, not measured server object sizes or download reservations; real headers and formats can differ.

`archive-acquisition.json` records every relevant full train/test archive URL, declared byte size, Git blob ID and LFS SHA-256 from the pinned official Hub tree API. These are metadata declarations, not local hash verification. Sixteen full archives total **16,113,568,959 bytes, about 15.01 GiB**, before models and metadata. They exceeded the initial 10 GiB allowance but fit under the current 100 GB ceiling. Train archives alone are 12,920,048,425 bytes; test archives are 3,193,520,534 bytes. The archive route still needs bounded acquisition, license/notice and selective-extraction review before any GET. The sixteen TSV bodies total 17,730,888 UTF-8 bytes per full metadata pass; planning performed repeated metadata requests but no audio requests.

The initial Viewer `/rows` and `/first-rows` requests for `fr_fr/train` hit a 300,000,000-byte parquet scan limit, with attempted scans of about 731 to 733 MB. A later `/filter` one-ID test succeeded for one `fr_fr/test` selection after an index-loading response; its Viewer row index differed from the pinned TSV row. The pinned TSV row plus original filename remains the frozen locator. Do not manufacture a direct per-file URL or assume that streaming a gzip archive only transfers the retained clips. A future acquisition step needs a verified bounded per-asset route or a reviewed full-archive extraction path. Do not silently weaken speaker separation by substituting validation for train.

## Required cases still missing

| Required case | Primary-source lead | Current decision |
| --- | --- | --- |
| Canadian French `fr-CA` | [WMT24++ card](https://huggingface.co/datasets/google/wmt24pp/blob/main/README.md) declares Apache 2.0 and the exact text-only subset `en-fr_CA`, split `train`, file `en-fr_CA.jsonl`. [CommissionsQC paper](https://www.isca-archive.org/interspeech_2025/serrand25_interspeech.pdf) describes Quebec speech aligned to official inquiry transcripts. | Neither is selected here. WMT24++ offers an English-to-Canadian-French text lead, not a Canadian French speech test or natural reverse-direction test. CommissionsQC's reviewed paper does not establish a downloadable licensed package suitable for this manifest; acquisition and terms need verification. |
| Navajo `nv` | [UNM Navajo Sound Profile](https://navajo.unm.edu/dinesound/html/main.html) supplies linguistic and pronunciation material with a Navajo Language Program copyright notice. | No explicit dataset reuse license or sentence-level ASR/reference package was established. Do not scrape audio based on visibility alone. This bounded search does not prove no suitable corpus exists. |
| Klingon `tlh` | [KLI sounds](https://www.kli.org/about-klingon/sounds-of-klingon/) supplies orthographic and pronunciation information. [Tatoeba terms](https://tatoeba.org/en/terms_of_use) distinguish textual sentences from contributor-specific audio licenses. | No qualified speech package selected. A future Tatoeba review must pin exact sentence/translation/audio IDs, authors and audio licenses individually, and group connected translations before splitting. Its text license cannot be assumed to cover audio. Do not substitute franchise dialogue or invented references. |

These remain substantive processing requirements. Honest unsupported results preserve the gap; they do not satisfy it. No new human reviewer recruitment is required by this plan. Published reference evidence and independent automated checks remain the intended evaluation basis.

## Verification scope

Metadata reads used URL-encoded parameters, explicit subset/split discovery, zero-based offsets, at most 100 rows per row-like request, and pagination/partial checks. No account token, Python, inference, audio download or paid request was used. The failed Viewer access was not bypassed with unbounded parquet fetching.

Local validation checks JSON parsing, 112 unique asset locators, exact 4/10 per-language counts, duration bounds, all required subsets, reference hash format, global split separation, and archive count/size. Promoted scripts and data were compared byte for byte with the reviewed scratch files before staging; Git normalizes text line endings. This checks the metadata plan only. Audio hashes, decoding, actual reference fidelity, acceleration and language quality remain unmeasured.
