# Offline FLEURS scorer prototype

Reviewed 2026-09-22. This standalone Rust 1.98.1 research package is outside the
product workspace and has `publish = false`. It scores local text artifacts only.
It does not acquire audio, extract corpus text, run models, make paid calls, or
contain a network client. No real speech quality result exists from this work.

## Frozen selection and input identity

The scorer accepts only the exact bytes of the canonical
[`../language-corpus/fleurs-screening-manifest.json`](../language-corpus/fleurs-screening-manifest.json), SHA-256
`487632ee83967f868afa6313d54f544a32cb07075d3b446701b05af870389fd9`.
The dataset is `google/fleurs` at revision
`70bb2e84b976b7e960aa89f1c648e09c59f894dd`.
That manifest contains only frozen selection metadata and text hashes, no
audio or reference sentences. Its original CC-BY-4.0 license declaration and
dataset source links are retained; the package license does not replace them.

All 112 assets must match the eight configurations and sentence groups:

- `ar_eg`, `cmn_hans_cn`, `en_us`, `es_419`, `fr_fr`, `hi_in`, `pt_br`, `sw_ke`.
- Calibration uses four train sentence groups per language, 32 clips total.
- Holdout uses ten test sentence groups per language, 80 clips total.
- Sentence groups are parallel across languages and disjoint between partitions.

Four plus ten clips per language are smoke screening, not qualification. French
here is `fr_fr`; Canadian French, Navajo and Klingon remain visible corpus gaps.
The dataset card says train speakers differ from dev/test speakers. This scorer
does not establish any broader speaker independence or pretraining exclusion.

Opaque clip IDs are `clip-` followed by lowercase SHA-256 of UTF-8
`sigy-fleurs-clip-v1`, one NUL byte, then the exact manifest `asset_id`. The mapping
command emits the selected partition's IDs, source locators and reference hashes
for the evaluator. Workers must receive only opaque clip IDs and their authorized
audio, never this mapping or reference artifacts.

Reference and candidate files are separate JSON envelopes for exactly one
partition. Every selected clip needs one record in each file. Omission is an
error, not an abstention. Duplicate, unknown or cross-partition IDs fail closed.
The CLI requires each file's previously recorded SHA-256 and verifies the bounded
bytes actually read. Every raw reference string must additionally match its
frozen manifest UTF-8 transcription hash. Hashes are integrity identities, not
signatures or proof of honest provenance. No references can be recovered from
these hashes, and this package does not attempt that.

## Local commands

From this directory:

```powershell
cargo fmt --check
cargo test --offline --locked -- --test-threads=2
cargo clippy --offline --locked --all-targets -- -D warnings
cargo build --offline --locked
cargo audit --no-fetch --no-yanked --file Cargo.lock
./target/debug/scorer-probe.exe selection ../language-corpus/fleurs-screening-manifest.json calibration
./target/debug/scorer-probe.exe selection ../language-corpus/fleurs-screening-manifest.json holdout
```

The checks above passed with 24 tests on the Windows research host. The cached
advisory check cannot establish that the local advisory database is current.
Keep local run receipts and evaluator-only outputs in ignored `.agents/`, not in
this source directory.

Scoring syntax after real evaluator-only references and candidate artifacts exist:

```text
scorer-probe score MANIFEST calibration|holdout REFERENCES REFERENCES_SHA256 CANDIDATES CANDIDATES_SHA256
```

Output is deterministic JSON to stdout, ordered by opaque clip ID and config.
No input is rewritten. Successful output includes both artifact digests, dataset
and selection identities, compiled source-bundle digest, declared profile digest,
policy version, raw text,
normalized text, per-clip edits, per-language results and overall micro counts.
Reports contain answers and are evaluator-only. Keep calibration and holdout
files, workers and reports in separate roots. A holdout report must not be fed
back into calibration or profile selection.

The minimal envelope shapes below are illustrative, incomplete synthetic
examples. They deliberately cannot pass the actual manifest/reference gates.
`MANIFEST_SHA256`, `PROFILE_SHA256` and `OPAQUE_CLIP_ID` must be replaced with
verified identities; all 32 or 80 records are required.

```json
{
  "schema_version": 1,
  "manifest_sha256": "MANIFEST_SHA256",
  "partition": "calibration",
  "records": [{ "clip_id": "OPAQUE_CLIP_ID", "text": "Synthetic example only." }]
}
```

```json
{
  "schema_version": 1,
  "manifest_sha256": "MANIFEST_SHA256",
  "partition": "calibration",
  "normalization_id": "nfc17-fixed-white-space-v1",
  "profile_sha256": "PROFILE_SHA256",
  "declared_tuning_partition": "calibration",
  "records": [{
    "clip_id": "OPAQUE_CLIP_ID",
    "outcome": {
      "status": "recognized",
      "text": "Synthetic example only.",
      "language": { "state": "unknown" }
    }
  }]
}
```

Recognized language evidence can be `unknown`, `known` with one `label`, or
`mixed` with 2 to 8 distinct `labels`. Labels are preserved provider assertions,
not parsed BCP 47 tags, canonical aliases, confirmed language correctness, or
route capability. Mixed and unknown recognized text is still scored. A known
provider label does not substitute for a measured language detector result.

Other outcomes are `abstained`, `failed` and `unsupported`, each with a nonempty
bounded `reason` and no text or language observation. They remain separate in
counts. Unsupported is an attempt outcome assertion, not an independent route
capability registry. A recognized empty string is reported separately from all
three. Unknown fields, including additional fields on an `unknown` language
object, duplicate JSON fields and impossible outcome fields are rejected. JSON
parsing and I/O failures use static diagnostics rather than echoing artifact
text or paths. The final stderr diagnostic escapes non-ASCII and control
characters and is bounded to 512 ASCII bytes plus the fixed prefix and newline.
The profile digest is a declared identity for a separately frozen
model/runtime/tokenizer/decoder/prompt configuration; this prototype does not
load or verify that configuration. Declared holdout tuning is refused, but a
declaration cannot prove isolation or prevent an operator from lying.

## Scoring contract

Policy `nfc17-fixed-white-space-v1` uses Unicode 17.0.0 NFC via
`unicode-normalization 0.1.25`. It then collapses each run of the fixed Unicode
White_Space property to one ASCII space and trims those runs at the ends. The
fixed set is U+0009..U+000D, U+0020, U+0085, U+00A0, U+1680,
U+2000..U+200A, U+2028, U+2029, U+202F, U+205F and U+3000.

CER counts normalized Unicode scalar values, including remaining spaces. It is
not grapheme-cluster error rate. WER splits only on normalized ASCII space.
Case, diacritics, punctuation, digits, scripts, joiners and other format
characters are retained. There is no NFKC, case folding, punctuation stripping,
transliteration, stemming, clitic splitting or numeral rewriting. The raw text
is separately retained without these scoring transformations.

| Configuration | Primary reading and explicit rule |
| --- | --- |
| `ar_eg` | WER with CER; retain harakat, tatweel and alef/hamza variants; no clitic segmentation. |
| `cmn_hans_cn` | CER primary; no simplified/traditional conversion. Whitespace WER is diagnostic only and may treat a whole sentence as one token. |
| `en_us` | WER with CER; retain case, punctuation, contractions and digit spelling. |
| `es_419` | WER with CER; retain accents, n-tilde and inverted punctuation; no regional rewriting. |
| `fr_fr` | WER with CER; retain accents, ligatures, elisions and hyphens; no Canadian French substitution. |
| `hi_in` | WER with CER; retain vowel signs, nukta, virama and joiners; scalar CER is not a count of visual characters. |
| `pt_br` | WER with CER; retain accents, cedilla, contractions and hyphens. |
| `sw_ke` | WER with CER; retain spelling and punctuation; no morphological splitting. |

These are conservative prototype rules, not claims of comparability with a
published benchmark's text normalizer. A normalization change needs a new policy
identity and re-scoring of the same immutable artifacts. A profile chosen from
calibration must be frozen before holdout; do not optimize normalization on
holdout results.

Unit-cost Levenshtein ties prefer diagonal, then deletion, then insertion.
Counts are substitutions, deletions and insertions. A rate is the exact integer
fraction `(S + D + I) / reference_units`, with no floating point or rounding.
Rates can exceed one. Zero-reference rates are null, including when candidate
insertions exist. Empty reference clips and empty references with insertions
are separately counted.

Each aggregate reports:

- All selected clips, scoring absent output as an empty candidate. This keeps
  abstentions, failures and unsupported attempts in reference denominators.
- Recognized-only counts and rates, conditional on recognition. Read these
  alongside outcome counts and recognized/total coverage, never alone.
- Exact normalized matches, including separate recognized-only exact matches.
  An empty-reference absent-output match is not a successful recognition.
- Known, mixed and unknown evidence counts among recognized clips.

Per-language micro counts are primary evidence. Overall micro counts are
descriptive and cannot establish that every language passed. Each clip is
aligned separately before aggregation, so words cannot match across clips.
There are no pass thresholds, confidence intervals, quality labels, language
confusion scores or claimed model confidence calibration in this prototype.

## Resource and trust boundaries

Each local JSON file is limited to 2 MiB, with at most one extra byte read to
detect overflow. Text is limited to 8192 UTF-8 bytes, 1024 raw and normalized
scalars, and 512 normalized tokens. Labels are at most 128 UTF-8 bytes; outcome
reasons at most 512. Record counts are exactly 32 or 80. An entire run has a
32,000,000 alignment-cell admission bound checked before any alignment. Dynamic
programming keeps two rows. Oversize work fails without a partial report.

These are structural limits, not measured CPU/RAM guarantees or a process
sandbox. File I/O is synchronous and has no hard wall-clock deadline. Select
ordinary local files: this program does not prevent a mapped filesystem,
reparse point or UNC path from causing operating-system network access. It does
not enforce worker isolation, inspect model training data, establish when a
profile was frozen, or authenticate artifact producers. Those gates remain in
the supervised evaluation harness. No real corpus quality or hardware capacity
claim follows from synthetic tests.

## Dependencies and verification

All builds in this task use cached crates and `--offline --locked`. Direct
dependencies are `serde 1.0.229`, `serde_json 1.0.151`, `sha2 0.11.0` and
`unicode-normalization 0.1.25`, each MIT or Apache-2.0. NFC adds the latter and
its `tinyvec 1.13.3` dependency (Zlib or Apache-2.0 or MIT); no dependency is added
to the product workspace. `Cargo.lock` freezes the full 23-package graph.
The source-bundle digest hashes the UTF-8 concatenation of `path:sha256` lines,
each ending in LF, in the fixed order in `source_bundle_digest`. These are the
bytes compiled into the executable; it does not reread mutable source files at
score time. Build receipts must separately pin the compiler and executable.

First-party code forbids unsafe and denies unwrap/expect. This is not an audit
of all transitive implementation code. The offline advisory check cannot
establish that the locally cached advisory database is current. Yanked-crate
checking is disabled to avoid registry access. An earlier development advisory
check used `--no-fetch` without `--no-yanked`; no network observation was taken,
so that earlier command is not proof of zero network access.

Tests use synthetic text only. They cover substitution/insertion/deletion,
combining forms, retained scripts/format characters, explicit whitespace,
empty references, rates above one, deterministic ties, a separate exhaustive
recursive edit-distance oracle, duplicate/missing/unknown clips, altered
references, partition leakage, wrong identities/policies, mixed/unknown states,
abstentions/failures/unsupported outcomes, illegal JSON fields, read and work
limits, and the actual metadata-only 112-asset manifest. No synthetic reference
can pass the frozen real-reference hash checks.

Primary source pointers for later review:

- [Frozen dataset card](https://huggingface.co/datasets/google/fleurs/blob/70bb2e84b976b7e960aa89f1c648e09c59f894dd/README.md)
- [Unicode normalization specification](https://www.unicode.org/reports/tr15/)
- [Unicode 17 property data](https://www.unicode.org/Public/17.0.0/ucd/PropList.txt)
- [Pinned normalization crate documentation](https://docs.rs/unicode-normalization/0.1.25/unicode_normalization/)

This task read cached crate sources and local corpus metadata only; these links
were not fetched during implementation.
