# Bounded offline judge capability runner

This Rust experiment uses only the already retained, pinned CPU runtime and
Gemma model from the [translation calibration](../calibration-32-translation.md).
It shares frozen reference, control and quote validation with the
[offline scorer](../../language-scorer/README.md#offline-judge-control-calibration).
It creates no product job, hosted request or paid reservation.

`freeze` accepts the existing 28-reference calibration envelope, verifies each
original/English pair, and builds 126 declared controls: two examples per nine
categories for seven languages. The controls cover boarding identity documents,
trade agreements and dates, and international festivals. They use three distinct
English sentences. The fourth existing sentence has a known Hindi subject
divergence and is not used. Repeated languages and mutations do not create
independent reference sentences. Canadian French, Navajo and Klingon remain
uncovered.

Reference text is derived from the retained FLEURS calibration subset. Preserve
the [dataset license and attribution packet](../../language-corpus/fleurs-subset-attribution.md).
The declared controls change English wording as recorded in their single-span
lineage; those changes are not publisher translations or independent annotations.

The categories include exact and formatting matches, equivalent expressions,
entity/date/negation changes, omitted assertions, unrelated fluent content, and
untrusted instructions. Single-span rewrite lineage is checked; semantic labels
remain experiment assertions rather than independent gold. The operational
critical label means a material meaning change here, not an established severity
annotation from a published error corpus.

Before inference, freeze pins the rubric, schema, raw no-thinking template,
opaque-ID order, all control bytes, fixed thresholds, source bundles, model and
every runtime file. The runtime archive must match its previous publisher hash;
each local file must match its named archive member. ZIP reads use the
[maintained Rust implementation](https://github.com/zip-rs/zip2) with Deflate
support only, bounded member counts and sizes, no extraction and no new asset
download. The OS/system-library closure is not qualified.

Frozen screening criteria require two controls per category per language, at
least nine critical detections among ten declared critical controls, no false
positive among eight declared acceptable controls, and at most a tenth of all
controls abstained. These strict debugging criteria intentionally expose misses
and false alarms. They are not statistically justified language-quality bounds.

```text
judge-runner freeze MANIFEST REFERENCES REFERENCES_SHA256 RUNTIME_DIR GEMMA_MODEL NEW_OUTPUT_DIR
judge-runner run MANIFEST FROZEN_OUTPUT_DIR PROFILE_SHA256
```

Keep output under gitignored `.agents/`. The runner refuses an existing freeze
directory or run marker. It does not resume, retry, tune or replace a previous
experiment. Keep all raw replies and receipts as evaluator-only material.

The Windows-only run uses one Job Object at a time: one process, two requested
CPU inference threads, 8 GiB committed memory, two whole logical processors of
CPU quota, a 90-second call deadline and a 45-minute batch deadline. It explicitly
disables GPU use and clears inherited
environment variables. Stdout and stderr have 64 KiB and 256 KiB ceilings.
The thread flag configures inference, not a Job Object thread-count ceiling.
The runtime also creates helper threads; aggregate CPU remains bounded by the
two-processor quota. No total OS-thread-count guarantee is claimed.
Successful completion, timeout, cancellation and output failures all require
kill/wait and an empty-group snapshot before publication. Unproven cleanup
aborts the batch. The absolute batch deadline starts before run validation;
the call deadline spans prompt/setup/spawn and collection. An expired deadline
admits no further native launch. Cleanup has separate bounded waits of up to
five seconds for root exit and five seconds for an empty group. Synchronous
filesystem operations can block and cannot be forcibly interrupted by these
deadlines; admission checks the remaining time after setup.
Complete collected stdout/stderr bytes are retained with hashes. Timeout,
cancellation and output-limit failures discard partial streams, explicitly
mark raw output incomplete, and never parse those bytes.
Application `--offline` is not OS network isolation, which
remains an explicit limitation.

The judge receives source, English reference, candidate and separate untrusted
context. Expected category, mutation lineage, thresholds, clip locators and
translator identity are hidden. Raw prompts omit a thinking system turn; the
[official model card](https://ai.google.dev/gemma/docs/core/model_card_4) describes
this thinking-token control. Temperature zero is a frozen deterministic
screening choice rather than the card's general recommended sampler.

Constrained JSON generation does not prove a sound answer. The runner converts
only nonempty, exact, uniquely occurring excerpts into UTF-8 byte ranges.
Invalid JSON, status, reasons, fabricated/ambiguous excerpts, native failures and
unattempted controls become explicit abstentions. A checked quotation proves
literal membership, not correct reasoning. All 126 controls remain in the final
denominator. The scorer reports language/category confusion and abstention counts.

[AutoMQM](https://research.google/pubs/the-devil-is-in-the-errors-leveraging-large-language-models-for-fine-grained-machine-translation-evaluation/)
and [GEMBA-MQM](https://aclanthology.org/2023.wmt-1.64/) motivate investigating error
spans, but do not establish this small quantized model's judge capability. The
existing Hy-MT2 model's [pinned card](https://huggingface.co/tencent/Hy-MT2-1.8B-GGUF/blob/a0c709d9fac510f2c807aa3af52872340dc37a4a/README.md)
describes specialized translation. It is not assumed to be a second judge. A
Gemma judgment of its own translation would not establish family independence.

The [2026-09-30 frozen 126-control run](../judge-capability-2026-09-30.md)
completed within its bounds with all native groups empty. Every source-language
group failed the declared capability criteria. This profile must not be used
as a qualified semantic judge. English candidates with supplied English
references can be compared without understanding their original language.

Local checks, from this directory:

```text
cargo fmt --check
cargo test --offline --locked -- --test-threads=2
cargo clippy --offline --locked --all-targets -- -D warnings
cargo build --offline --locked
cargo audit --no-fetch
```

The experiment requires a separate native containment check before model launch.
Do not run inference concurrently with heavy compilation or benchmarking.
Passing constructed controls does not qualify held-out or broadcast translation;
do not inspect frozen holdout references to improve this screen.

This isolated crate is outside the root workspace coverage collection. The
recorded tests, freeze and single real run measured 1086/1176 owned lines,
92.34%, with all test source included and no exclusions. Preserve the complete
LLVM JSON and validate it from the repository root with the shared Rust gate:

```text
cargo run --locked -p sigy-xtask -- verify-coverage-report research/experiments/local-mt/judge-runner/Cargo.toml .agents/judge-runner-coverage-final.json
```

The [experiment record](../judge-capability-2026-09-30.md#verification-receipts)
contains the actual collection scope and report hashes. The gate checks an
existing report; it does not repeat inference or silently include this crate
in root workspace tests.
