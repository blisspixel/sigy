# Reference-assisted local judge capability screen

Review and run date: 2026-09-30. This is an offline research capability screen
using the existing Windows CPU runtime and quantized Gemma model. It creates no
product job or paid request. External spend is USD 0. It does not establish
translation quality, independent semantic gold, held-out performance or a
qualified language.

The [bounded Rust runner](judge-runner/README.md) shares validation and scoring
with the [offline scorer](../language-scorer/README.md). The retained assets and
published parallel-reference lineage are those of the
[previous translation calibration](calibration-32-translation.md). Required
[FLEURS legal attribution](../language-corpus/fleurs-subset-attribution.md)
remains separate from the declared experimental mutations.

## Frozen method

Before any model call, the runner verified all 28 existing non-English
calibration reference pairs against the immutable corpus manifest. It then
froze 126 controls: two variants of nine categories for each of seven languages.
The categories are unchanged, formatting, equivalent wording, changed entity,
changed quantity/date, changed negation, omitted assertion, unrelated fluent
content and separate untrusted instructions. Each language has ten declared
critical and eight declared acceptable controls.

Controls use sentence groups 831, 264 and 773, covering boarding identification,
trade agreements and dates, and arts/sciences festivals. These are three
distinct English references repeated across languages. Group 1087 is excluded
consistently because its Hindi reference has the previously recorded subject
divergence. References were not silently corrected. Canadian French, Navajo and
Klingon are absent. No frozen holdout reference text was read.

The runner records byte-exact single-span mutation lineage. This verifies what
changed, not whether its declared semantic label is correct. The equivalent
wording controls are additional declared negative controls, not independent
paraphrase annotations. The operational critical label means material meaning
change in this experiment; it is not a published MQM severity annotation.

The judge sees source, English reference, candidate and separate untrusted
context. Opaque IDs and sorted opaque-ID order hide the clip locator, expected
category, lineage and thresholds. The English reference remains visible, and
prior model exposure to the dataset is unknown. There is one fixed answer order
and one model family. No independent-family or order-variation claim follows.
All constructed candidates are English and the English reference is supplied.
A model can distinguish these mutations through English reference comparison
alone. Grouping rates by source language does not prove that it understood or
used the original language; checked source quotations prove literal membership
only. Even a perfect result on all 126 controls cannot qualify a multilingual
judge or translation quality.

Thresholds were frozen before inference: at least two examples of each category
per language, at least nine critical detections out of ten, zero false positives
out of eight acceptable controls, and at most one tenth of all controls
abstained. All critical abstentions remain in the sensitivity denominator.
These are deliberately strict capability checks, not statistically justified
language-quality bounds.

The raw no-thinking template, rubric and JSON schema are pinned. Temperature
zero, seed zero, two requested inference threads, context 4096 and at most 256 generated tokens are
fixed screening choices. GPU use is disabled. Primary sources reviewed were the
[official model card](https://ai.google.dev/gemma/docs/core/model_card_4),
[pinned runtime argument implementation](https://raw.githubusercontent.com/ggml-org/llama.cpp/7fe450e19305b828c199d602c23a8337aaa1f03b/common/arg.cpp),
[AutoMQM](https://research.google/pubs/the-devil-is-in-the-errors-leveraging-large-language-models-for-fine-grained-machine-translation-evaluation/)
and [GEMBA-MQM](https://aclanthology.org/2023.wmt-1.64/). Their larger-model results
do not qualify this small local model. The retained translation-specialized
[Hy-MT2 model card](https://huggingface.co/tencent/Hy-MT2-1.8B-GGUF/blob/a0c709d9fac510f2c807aa3af52872340dc37a4a/README.md)
does not establish judge capability, so it was not added as a second judge.

## Containment and interpretation

Each admitted call uses a fresh Windows Job Object: one process, 8 GiB committed
memory and two whole logical processors of CPU quota. Each call has an absolute
90-second deadline spanning prompt setup, spawn and collection. The whole
batch has a 45-minute deadline beginning before profile and asset validation.
The `-t 2` argument configures inference threads, not a total OS-thread ceiling.
A read-only live snapshot observed six runtime OS threads, including helper
threads. The aggregate Job Object CPU quota remains two logical processors.
No native process starts after expiry. Cleanup is bounded separately by five
seconds for root exit and five seconds for an empty-group snapshot. Synchronous
filesystem operations remain cooperative and cannot forcibly interrupt a
blocked read or write. OS network isolation is unproven; application offline
mode is not that proof.

Every local runtime file is hashed and compared with its named member in the
previously pinned upstream ZIP, both at freeze and before run. There are 52
members. The model, archive, runner binary, source bundles, schema, rubric,
template and control order are hashed. The system/driver DLL closure is outside
this experiment's qualification.

Stdout and stderr ceilings are 64 KiB and 256 KiB. Complete collected streams
are retained byte for byte with hashes only after the same Job Object is proven
empty. Timeout, cancellation and output failures discard partial streams and
mark raw output incomplete. Such bytes are never parsed. An unproven empty
group aborts publication. Invalid schema, status, reason, UTF-8 or fabricated or
ambiguous quotation becomes an abstention. A checked quote proves literal
membership in one unique UTF-8 range, not sound reasoning. Unattempted controls
also remain explicit abstentions in the full 126-control denominator.

This is one predeclared run. No control was retried, and no prompt, schema,
rubric, order or threshold was tuned after inspecting model replies. The native
success, timeout and output-overflow fixtures check runner contracts only and
make no semantic claim.

## Reproducibility identities

The private freeze directory is
`.agents/language-evaluation/mt/judge-capability-2026-09-30`. It retains profile,
controls, blinded inputs, rubric, schema, raw prompts/replies and attempt receipts.
The exact instrumented executable is also retained as
`runner-instrumented-frozen.exe`, with a copy hash checked against the frozen
profile before any subsequent build can replace the original target artifact.
The freeze completed at Unix second `1790799341`, before inference.

| Object | SHA-256 |
| --- | --- |
| Frozen profile | `4ff2cb436b33115074a9e988a242ad0b7a1c690e8582492768077601588c5a3e` |
| Controls | `325713cf3435b5d095a3a9058211776c76528cecaabae66d5cb0a6aa6da944cc` |
| Blinded inputs | `e93c5ae8ee5aa1a11f72912d5889663e140a5d49b8aa010acba4666aa0cabd28` |
| Rubric | `1152a6a2f69173fa3ae8809f757bd82d6b6bab16b86651e3817034c426f28531` |
| Output schema | `c77b04ffe36898517df197ba30f1cd3049c981c841ba1382a443a86258ccd38e` |
| Raw prompt template | `eebc9e4c5d24fe2f1f626f0093de8566e3ae84d71f4220c6cafd39e15e57b618` |
| Control order | `2c36e5d1a426af179742e42bc20e506175c13e56effabebf75d8383cb101d302` |
| Frozen runner source bundle | `21f64d3215dc1117f5988c1ff5d4871ea47fbd1ce9406f00b127ece5a4a08650` |
| Frozen scorer source bundle | `72482df2d994c9c7ca4a1ed40085240caec8ad4d4d5c1c8396314665f03ea0d9` |
| Instrumented runner executable | `16430c83115c2e85f9e18db910da152a6c0c33dcec3ea3e4e6d5fbb639a24df2` |
| Complete judgment artifact | `9ef448724a712e49d9b736dcb1ec1626c901a30aa48d6f9c1957f2eef5943b54` |
| Offline score report | `2aa343342a96f3f1b032d7c7b7e82c6943acbdd97155759ea440b26e9a8795a6` |

The actual executable was a `cargo-llvm-cov` instrumented debug build. It is
hashed separately from source; this run is not a release-build speed benchmark.
The full frozen profile carries absolute local paths, every runtime member
hash, argument vector and resource bounds. Raw reference/candidate contents
stay evaluator-only.

## Results

Exactly 126 native calls completed before the original 45-minute deadline.
There were no retries, unattempted controls, native timeouts, nonzero exits or
unproven cleanup outcomes. All 126 receipts prove the corresponding Job Object
empty and retain complete stdout/stderr hashes. The runner accepted 64 replies
as structurally valid literal-evidence judgments; every one said `critical`.
The other 62 became explicit invalid-response/evidence abstentions. No reply
was accepted as `acceptable` or a model-declared abstention.
Read-only raw inspection distinguishes proposal from accepted outcome: 59
rejected replies declare `acceptable` but supply nonempty quotations, violating
the frozen rubric's empty-quote contract. Three rejected replies declare
`critical` but fail literal evidence checks. These observations do not relax
the parser or promote rejected answers retrospectively. The profile did propose
acceptable answers; their format-contract failure prevented acceptance.

Each language has ten declared critical and eight declared acceptable controls:

| Source language group | Critical detections / 10 | False positives / 8 | Critical abstentions | Acceptable abstentions | Frozen criteria |
| --- | --- | --- | --- | --- | --- |
| Egyptian Arabic | 7 | 1 | 3 | 7 | Fail |
| Mandarin | 8 | 1 | 2 | 7 | Fail |
| Latin American Spanish | 9 | 1 | 1 | 7 | Fail |
| France French | 9 | 1 | 1 | 7 | Fail |
| Hindi | 6 | 1 | 4 | 7 | Fail |
| Brazilian Portuguese | 9 | 1 | 1 | 7 | Fail |
| Swahili | 9 | 1 | 1 | 7 | Fail |

Totals are 57/70 critical detections, 7/56 false positives, 13 critical
abstentions and 49 acceptable abstentions. There are zero true negatives and
zero explicit false negatives; the missed critical controls are abstentions,
which remain in the sensitivity denominator. Every language fails both the
zero-false-positive requirement and the abstention ceiling. The full private
score report also preserves counts by language and category.

| Category, 14 controls each | Critical detections | False positives | Abstentions |
| --- | --- | --- | --- |
| Unchanged | 0 | 0 | 14 |
| Formatting | 0 | 0 | 14 |
| Equivalent wording | 0 | 0 | 14 |
| Entity change | 13 | 0 | 1 |
| Quantity/date change | 14 | 0 | 0 |
| Negation change | 14 | 0 | 0 |
| Omission | 4 | 0 | 10 |
| Fluent unrelated content | 12 | 0 | 2 |
| Untrusted instruction context | 0 | 7 | 7 |

The seven false positives all occur on instruction-context controls whose
candidate text remains unchanged. Ten omitted-assertion controls abstain.

This frozen runtime/model/prompt/schema profile is unsuitable for quality
judging under the declared criteria. Valid quotations did not prevent false
alarms on untrusted instructions. English-only candidates with visible English
references did not produce a contract-valid acceptable judgment. The result
does not identify whether sampling, structured decoding, instructions or model
capability caused the pattern. It
does not establish multilingual comprehension or translation quality.

Cumulative call wall time was 2,534,171 ms (42 minutes 14.171 seconds), mean
20,112.47 ms and maximum 51,581 ms. The highest measured Job Object peak
committed memory was 2,094,465,024 bytes. Total contained CPU time was
4,504,171,875 microseconds. Those figures come from the empty-group snapshots;
they are not a host budget. Cumulative call wall time excludes initial closure
validation and journal overhead. The whole run still enforced its original
absolute deadline. These instrumented-run measurements are not a release-build
benchmark or a simultaneous-capacity claim.

Do not apply this profile to previous translation outputs or open the holdout
as a follow-up to this failed screen. Next evidence needs a separately frozen
capability profile, fresh controls with broader published reference diversity,
answer-order variation and a genuinely independent family where task capability
is established. The three English references and absent required cases remain
open limitations.

## Verification receipts

Before freeze, the runner passed ten contract tests, including contained native
success, timeout and output overflow, and warnings-denied all-target Clippy.
The ignored test is a child fixture invoked explicitly by the containment test,
not an omitted production-path test. The scorer passed 47 binary tests and 16
library test executions plus warnings-denied all-target Clippy. Cached advisory
checks passed; their cache does not prove current advisory freshness.

The instrumented test and CLI runs collect source coverage without filename,
package or test-source exclusions. Separate full LLVM JSON reports and the
shared Rust `verify-coverage-report` command validate each isolated manifest;
the root workspace gate does not implicitly include either research crate.

Both isolated packages passed the shared Rust exact-integer threshold gate:

| Package | Covered / instrumented owned lines | Display percentage |
| --- | --- | --- |
| `sigy-judge-runner` | 1086 / 1176 | 92.34% |
| `scorer-probe` | 2247 / 2384 | 94.25% |

These counts include test source. Runner coverage combines contract/native
fault tests, freeze and the one real model batch. Scorer coverage combines its
tests and the actual offline judgment-scoring CLI. Coverage is code execution
evidence, not semantic correctness. The collection disabled `cfg(coverage)`
and `cfg(coverage_nightly)` overrides; no source was omitted to raise a figure.

The private complete reports are `.agents/judge-runner-coverage-final.json`
(SHA-256 `608aa44565871040afea4f21a07283908bda94712552cb1fdb39ac697ab36e82`)
and `.agents/language-scorer-coverage-final.json`
(SHA-256 `b1670985e2ae011d4333d9f86ba8a90f12d71d287d366963158b3e24a7d6c76c`).
The gate counts report files owned by each supplied isolated workspace;
dependencies in a complete report do not become that package's covered lines.
Neither collection provenance nor source completeness can be authenticated
solely from the exported JSON, so the exact collection scope is stated here.

Actual collection used `cargo llvm-cov --manifest-path MANIFEST --offline
--locked --no-report --no-cfg-coverage --no-cfg-coverage-nightly --
--test-threads=2`, then instrumented CLI invocations through `cargo llvm-cov
run` with `--no-clean --json --include-build-script --output-path REPORT` and
the same cfg overrides. Both final exports used the following options, followed
by the shared checker from the repository root:

```text
cargo llvm-cov report --manifest-path research/experiments/local-mt/judge-runner/Cargo.toml --json --include-build-script --no-default-ignore-filename-regex --output-path .agents/judge-runner-coverage-final.json
cargo llvm-cov report --manifest-path research/experiments/language-scorer/Cargo.toml --json --include-build-script --no-default-ignore-filename-regex --output-path .agents/language-scorer-coverage-final.json
target/debug/sigy-xtask.exe verify-coverage-report research/experiments/local-mt/judge-runner/Cargo.toml .agents/judge-runner-coverage-final.json
target/debug/sigy-xtask.exe verify-coverage-report research/experiments/language-scorer/Cargo.toml .agents/language-scorer-coverage-final.json
```

The checker is also available through `cargo run --locked -p sigy-xtask --
verify-coverage-report MANIFEST REPORT`. It measures existing receipts and
runs no inference. Repeating model calls merely to collect coverage would be a
new experiment and is not part of this record.
