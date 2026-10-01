# Offline FLEURS scorer prototype

Reviewed 2026-09-30. This standalone Rust 1.98.1 research package is outside the
product workspace and has `publish = false`. It scores local text artifacts only.
It does not acquire audio, extract corpus text, run models, make paid calls, or
contain a network client. It now scores translation artifacts and declared
critical-error judge controls as well as recognition text. The
[calibration artifact replay](../local-mt/offline-validation-2026-09-30.md)
checks the translation implementation against existing real outputs; it does
not qualify language quality or a semantic judge.

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

The checks above passed with 47 binary tests and 16 library test executions on
the Windows research host. The shared modules supply both targets. The cached
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

The original 2026-09-22 ASR increment read cached crate sources and local corpus
metadata only; the pointers above were not fetched in that increment. The
2026-09-30 translation increment reviewed the pinned metric sources cited below.

## Translation screening

```text
scorer-probe mt-score MANIFEST calibration|holdout REFERENCES REFERENCES_SHA256 CANDIDATES CANDIDATES_SHA256
```

The command shares the ASR command's bounded, digest-checked local reads and
static parsing diagnostics. Separate strict envelopes use schema version 1,
the frozen `manifest_sha256`, and exactly one `partition`. Translation requires
28 calibration or 70 holdout records: all seven non-English configurations,
with four or ten clips each. English source clips are refused. Unknown fields,
duplicate fields, duplicate clips, omissions, cross-partition records and
altered references fail before output.

Each reference record has `clip_id`, `source_text` and `english_text`. The
original-script string must match the source clip's frozen transcription hash;
English must match the frozen `en_us` clip for the same sentence group and
partition. Each candidate has `clip_id`, `input_text`, `input_sha256` and an
`outcome`: `translated` with `text`, or `abstained`, `failed`, or `unsupported`
with a bounded `reason`. A translated empty string is counted separately.

The candidate envelope also requires `profile_sha256`,
`declared_tuning_partition: "calibration"`, the fixed `metric_signature` below,
and `input_origin`. `{"kind":"reference_text"}` requires every candidate input
to equal its verified published source. `{"kind":"recognized_text",
"recognition_profile_sha256":"...","recognition_artifact_sha256":"..."}`
binds the experiment's declared recognizer identities; each actual input string
is independently hashed. These identity declarations do not load the named
profile, authenticate its producer, or verify the external recognizer artifact.
Record those provenance checks in the experiment receipt.

Metric signature:

```text
chrF2++|case:mixed|eff:yes|nc:6|nw:2|space:no|raw-text-v1
```

The implementation follows the
[pinned upstream chrF implementation](https://github.com/mjpost/sacrebleu/blob/c596d9d2072a8f84200574a7a5c56c618e8d37e8/sacrebleu/metrics/chrf.py),
reviewed 2026-09-30: character orders 1 to 6, word orders 1 and 2, beta 2,
effective-order averaging, mixed case, and character whitespace removal. Word
splitting uses a fixed whitespace set matching the reviewed upstream split
behavior, and splits one trailing ASCII punctuation character, otherwise one
leading character. Raw Unicode forms and case remain unchanged. The ASR NFC
normalizer checks text bounds but its normalized strings do not enter chrF++.

Reports retain exact integer hypothesis/reference/match counts for all eight
orders, derived floating-point corpus and clip scores, artifact and compiled
source identities, input origin, coverage and the number of distinct English
reference strings. Corpus scores aggregate per-clip counts; they are not means
of sentence scores and n-grams never cross clip boundaries. All-clips scores
include absent output as empty text. Translated-only scores are conditional;
zero translated clips produce a null conditional score. Four English sentences
repeated across seven languages remain four references, not 28 independent
sentences. No chrF++ threshold establishes semantic correctness, critical-error
detection, language qualification, or model ranking.

Each text retains the existing 8192-byte, 1024-raw/normalized-scalar bound. At
most 70 pairs keep each aggregate n-gram count below 71,680. Tests check the
largest admitted counts, an independently hand-computed eight-order example,
Unicode forms, punctuation, empty outputs, provenance and hostile envelopes.
The real calibration replay reproduced all 32 prior per-language and overall
scores without a difference at stored floating-point precision.

## Offline judge-control calibration

```text
scorer-probe judge-inputs MANIFEST CONTROLS CONTROLS_SHA256
scorer-probe judge-score MANIFEST CONTROLS CONTROLS_SHA256 JUDGMENTS JUDGMENTS_SHA256
```

These commands execute no model, access no provider and incur no inference
charge. They prepare blinded inputs and evaluate separately recorded answers.
They accept only calibration, never holdout. Pin control and criterion bytes
before requesting any judgments; tool-side declarations cannot establish when
that freeze occurred. Judge dispatch and independent-family comparison remain
separate work.

The control envelope requires schema version 1, frozen manifest identity,
`partition: "calibration"`, `rubric_sha256`, `criteria`, and `controls`.
Criteria specify `minimum_per_category` (1 to 64) and three exact fractions
with `numerator` and `denominator`: `minimum_sensitivity`,
`maximum_false_positive_rate`, and `maximum_abstention_rate`. Denominators are
1 to 1000; fractions must lie between zero and one. The tool imposes no chosen
quality thresholds; the experiment must freeze and justify them.

Each control has an opaque lowercase SHA-256 `control_id`, frozen non-English
`clip_id`, verified `source_text` and `english_reference`, `output_text`,
`untrusted_context`, `category`, and `label_basis`. Categories are `unchanged`,
`meaning_preserving_formatting`, `meaning_preserving_paraphrase`, `entity`,
`quantity`, `negation`, `omission`, `fluent_unrelated`, and
`instruction_injection`. Unchanged, formatting, paraphrase and injection are
acceptable controls; the other five declare critical mutations.

`label_basis` uses `kind: "unchanged"` for exact-reference output;
`kind: "formatting_only"` requires changed raw bytes with the same fixed NFC
and whitespace normalization. `kind: "deliberate_mutation"` requires a
`reference_range` and `replacement_text`; replacing that exact English range
must produce the entire candidate output. These checks prove string lineage,
not the truth of a declared semantic category or severity. Paraphrase controls
also require this single-span rewrite lineage; their acceptable-meaning label
is a declared assertion, not independently validated by the string check. Mutation selection
still needs published references and a documented rubric. Injection controls
keep output unchanged and require a nonempty separate untrusted context.

`judge-inputs` excludes clip locators, expected categories, label bases and
thresholds. It emits opaque IDs, source and English reference text, candidate
output and untrusted context. These are judge inputs; they must never become
translator-worker inputs. Model identity is absent from each blinded item.

Judgment envelopes require schema version 1, the exact
`control_artifact_sha256`, `rubric_sha256`, `judge_profile_sha256`, bounded
`judge_family`, and exactly one record per control. Outcomes are `acceptable`,
`abstained` with `reason`, or `critical` with `source_quote`, `reference_quote`,
optional `output_quote`, and `reason`. Quotes have `start_utf8_byte`,
`end_utf8_byte` and `text`; every supplied quote must match a nonempty literal
UTF-8 byte range, including valid character boundaries. An omission may have
no output quote. A literal quote check does not establish valid reasoning.

Reports show exact confusion and abstention counts by language and category.
Sensitivity includes critical abstentions in its denominator. False-positive
rate uses all acceptable controls, and an independent abstention ceiling
prevents passing through mass abstention. Every non-English screening language
appears; absent languages fail coverage. Passing all declared control criteria
is explicitly distinct from qualifying that language or judging real model
outputs. The [bounded reference-assisted runner](../local-mt/judge-runner/README.md)
uses these shared Rust validation contracts; its experiment evidence remains
separate from product or language qualification.

The input cap remains 2 MiB and controls are limited to 448 total. Reference
hashes, text bounds, labels, identities, quoted ranges, reasons and fractions
are checked before a report. Tests are synthetic boundary fixtures; they cover
false positives, retained abstentions, missing languages/categories, blinded
output, mutated lineage, calibration-only admission and fabricated UTF-8
quotes. They establish no semantic sensitivity or false-positive claim.

The [2026-09-30 reference-assisted capability screen](../local-mt/judge-capability-2026-09-30.md)
ran and scored 126 frozen controls. Every language group failed its criteria;
it did not qualify a semantic judge. All candidates were English with English
references visible, so grouping by source language does not prove multilingual
understanding.

This isolated crate measured 2247/2384 owned lines, 94.25%, from instrumented
tests and the real offline judgment-scoring CLI, with test source included and
no exclusions. It is outside root workspace coverage collection. Preserve the
complete LLVM JSON and validate it from the repository root:

```text
cargo run --locked -p sigy-xtask -- verify-coverage-report research/experiments/language-scorer/Cargo.toml .agents/language-scorer-coverage-final.json
```

The [experiment verification record](../local-mt/judge-capability-2026-09-30.md#verification-receipts)
preserves the collection scope and report hash. A passing coverage gate proves
execution coverage, not language quality or semantic correctness.
