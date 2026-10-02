# Bounded hosted evaluation

Reviewed 2026-10-02. This standalone Rust 1.98.1 research package is outside
the product workspace and has `publish = false`. It sends bounded OpenRouter
requests for the [paid validation allocation](../../../docs/development/language-pipeline.md#paid-validation-allocation)
recorded in [progress](../../../docs/development/progress.md#spending-ledger).
It does not change any product crate, and it is not the product's provider
dispatcher. The [dated experiment record](hosted-evaluation-2026-10-02.md)
holds manifests, liabilities, results and limitations.

## What it reuses

- Exact rate and reported-cost parsing from `sigy-core::pricing` (decision
  [0037](../../../docs/decisions/0037-exact-provider-pricing.md)). Catalog
  rates with more than 18 fractional digits round up by one attodollar.
- The frozen FLEURS selection, reference validation, chrF++, CER/WER and the
  judge-control scorer from the [offline scorer](../language-scorer/README.md).
- The unchanged reply parser of the [frozen local judge screen](../local-mt/judge-runner/README.md),
  compiled from the same `response.rs` file, and its rubric and output schema.
- BLEU is not in the frozen scorer. `src/bleu.rs` ports the 13a corpus BLEU of
  the disposable study scorer and reproduces the published sacrebleu test values
  and every historical BLEU value of the [translation calibration](../local-mt/calibration-32-translation.md).

## Data destination

Plans accept only the frozen calibration partition: published FLEURS source
and English text whose hashes match the selection, the 126 constructed controls
derived from it, recorded local outputs for those items, and decoded calibration
WAV files whose names map to a selected asset. Holdout material, user
recordings and station captures cannot enter a plan. One contract probe per
route uses a synthetic sentence or the first English calibration clip.

Every request carries `provider.only`, `allow_fallbacks: false`,
`require_parameters: true`, `data_collection: "deny"`, `zdr: true` where the
route sets it, and `max_price` at the snapshot's highest listed prompt and
completion rate plus 5%. The serving provider and routing summary are recorded
for every settled request.

## Liability and the ledger

`plan-*` commands write a new directory with each exact request body, an
evaluator-only item list and a manifest. For every request the manifest holds:

- a prompt bound of body bytes plus 4096 tokens, capped at the endpoint context;
- a completion bound of the route's `max_tokens` (which providers document as
  covering reasoning and visible output), or the endpoint output ceiling;
- the highest rate of every matched endpoint, including service-tier variants
  and every pricing override that could apply, plus 5% headroom;
- the reservation, rounded up to one micro-USD, and a hard bound that uses the
  endpoint output ceiling and one web-search unit.

`run` dispatches one frozen manifest sequentially. Before each send it refuses
replay of a request already in the ledger, refuses a reservation that would
exceed the batch allocation, the USD 18 software cap, or (with the hard bound)
the USD 20 ceiling, verifies the body hash, and appends the reservation. After
the send it appends exactly one outcome:

| Outcome | Ledger effect |
| --- | --- |
| HTTP 200 with generation ID and `usage.cost` | Settled at the reported cost, rounded up |
| Any other response, timeout or interrupted read | Uncertain: the full reservation stays outstanding |
| Failure before a connection | Released |
| Cost or token count above its bound | Settled, plus a breach that freezes every batch |

Three consecutive unsettled outcomes, an HTTP 401 to 403, a breach, an
admission refusal or the 90-minute batch deadline stop the batch. There are no
retries. `reconcile` settles uncertain requests through the free generation
lookup when a generation ID is known; `key` records the key's limit, remaining
credit and usage, never its label. The ledger is append-only JSON lines with a
SHA-256 chain, synced per event, behind a lock file.

The key is read from `OPENROUTER_API_KEY` in the process environment at
dispatch time. It is never printed, logged, stored or committed.

## Commands

```text
hosted-eval init LEDGER
hosted-eval allocate LEDGER BATCH MICRO_USD NOTE
hosted-eval status LEDGER
hosted-eval key LEDGER
hosted-eval reconcile LEDGER
hosted-eval plan-probe ROUTE CATALOG ENDPOINTS SELECTION AUDIO_DIR OUT judge|translate|recognize
hosted-eval plan-controls ROUTE CATALOG ENDPOINTS SELECTION CONTROLS CONTROLS_SHA256 OUT
hosted-eval plan-outputs ROUTE CATALOG ENDPOINTS SELECTION REFERENCES REFERENCES_SHA256 OUT LABEL=PATH=SHA256...
hosted-eval plan-translate ROUTE CATALOG ENDPOINTS SELECTION REFERENCES REFERENCES_SHA256 OUT [RECOGNIZED RECOGNIZED_SHA256]
hosted-eval plan-recognize ROUTE CATALOG ENDPOINTS SELECTION AUDIO_DIR OUT CONFIG...
hosted-eval run LEDGER BATCH PLAN MANIFEST_SHA256
hosted-eval collect-controls PLAN MANIFEST_SHA256 SELECTION CONTROLS CONTROLS_SHA256
hosted-eval collect-outputs PLAN MANIFEST_SHA256
hosted-eval collect-translations PLAN MANIFEST_SHA256 SELECTION REFERENCES REFERENCES_SHA256 [RECOGNIZED RECOGNIZED_SHA256]
hosted-eval score-translations SELECTION REFERENCES REFERENCES_SHA256 CANDIDATES CANDIDATES_SHA256 OUT
hosted-eval recognition-references SELECTION REFERENCES REFERENCES_SHA256 OUT
hosted-eval collect-recognition PLAN MANIFEST_SHA256 SELECTION RECOGNITION_REFERENCES SHA256
hosted-eval baseline-recognition RESULTS RESULTS_SHA256 PROFILE SELECTION RECOGNITION_REFERENCES SHA256 OUT_DIR
```

Only `key`, `reconcile` and `run` contact the network, and only `run` makes
paid requests. Keep the ledger, snapshots, plans, raw responses and reports in
the main checkout's ignored `.agents/hosted-eval/`. Route profiles in `routes/`
pin each endpoint snapshot by hash.

The frozen selection hash covers the CRLF bytes of
`../language-corpus/fleurs-screening-manifest.json` in the original checkout.
Git stores that file with LF endings, so a fresh checkout does not match the
frozen hash; pass the original checkout's file. The unit tests restore CRLF
before checking the hash.

## Verification

From this directory:

```text
cargo fmt --check
cargo test --offline --locked -- --test-threads=2
cargo clippy --offline --locked --all-targets -- -D warnings
cargo build --offline --locked
cargo audit --no-fetch --file Cargo.lock
```

Offline fault fixtures cover refusal before send when an allocation is
insufficient, no resend on replay, retained liability for timeouts, HTTP errors
and missing usage, idempotent and conflicting settlement, release only before
connection, breach freezing, hash-chain tampering, partial lines, concurrent
writers, a tampered request body, and a loopback HTTP server for success,
oversized, truncated, hung, closed and refused connections. Coverage is
collected with `cargo llvm-cov` over the tests and the actual CLI runs; the
record states the measured figure. Coverage is execution evidence, not
semantic correctness.
