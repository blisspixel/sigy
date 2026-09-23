# Decision models and local classifiers

Reviewed: 2026-09-20. Status: candidate research and proposed evaluation. No model, SDK, or implementation language selected. No inference or paid request performed.

## Fit for Sigy

A small decision stage can classify transcript passages against a monitor's topic, distinguish a substantive discussion from a passing mention, categorize identified music using supported metadata, or prioritize material for a larger synthesis model. It should produce inspectable observations, with abstention and versioned criteria. It does not replace speech recognition, translation, song identification, evidence validation, or statistical computation.

The proposed pipeline is local capture and recognition, optional translation, inexpensive candidate selection, bounded semantic classification, deterministic aggregation, and evidence-linked synthesis. Evaluate whether classification actually reduces total cost and delay without unacceptable missed material. Adding a model to every block is not automatically an optimization.

## What Jev currently provides

TypeSafe introduced System One models and Jev on September 15, 2026 as models specialized for structured decisions rather than free-form generation. The launch benchmarks and speed comparisons are vendor evidence, not measurements on Sigy's radio workload or independent ground truth. [Announcement](https://typesafe.ai/blog/introducing-system-one-models-and-jev).

The current first-party model is `jev-1.13.0`. It accepts text, including structured text, with a 64k total request budget and a separate 32k bound for state plus the longest question. Audio, image, and video inputs are not supported. English is its strongest documented language; other languages require workload-specific evaluation. The listed price is $0.042 per million input tokens, with no output-token charge. First-party data-handling claims and enterprise retention options do not establish the policy of every routed integration. [Model reference](https://docs.typesafe.ai/models).

| Question type | Documented meaning | Proposed Sigy use |
| --- | --- | --- |
| Choice | Select among supplied categories with a distribution and confidence | Main topic, with an explicit other/insufficient-evidence option |
| Score | Distribution over defined rubric levels and a resulting score | Degree of substantive topic relevance |
| Noul | Probability assigned to a yes/no proposition | Whether a passage discusses a specified event |

Questions share the supplied state and are evaluated independently; later questions cannot depend on earlier answers within the same call. Question-map keys are identifiers, not instructions to the model. Put the actual condition and boundary cases in the question. [System One concepts](https://docs.typesafe.ai/concepts/system-one), [API reference](https://docs.typesafe.ai/api).

Choice/Score confidence and answer probabilities have different meanings; Noul has no separate confidence field. A high score is not a verified fact or a universal estimate of correctness. Preserve raw outputs and calibrate each task, language, and model profile on held-out examples. [Confidence reference](https://docs.typesafe.ai/confidence). Neural classification scores can need calibration even when label accuracy is strong. [Calibration research](https://proceedings.mlr.press/v70/guo17a.html).

Jev's documented limitations include counting, arithmetic, date comparisons, excessive irrelevant context, adversarial content, and logical consistency between independently asked questions. Keep calculations and authorization in deterministic application operations. Structured output constrains format; it does not guarantee a correct interpretation. [Version-specific limitations, reviewed upstream September 17, 2026](https://docs.typesafe.ai/model-jaggedness/jev-1.13).

## Verified OpenRouter integration surface

The official integration documents `POST https://openrouter.ai/api/v1/systemone`, using an OpenRouter key. This is a dedicated decision endpoint. The documented request contains `model`, `state`, and `questions`; responses include answers and usage, with OpenRouter adding request identity, provider, and cost. `jev-1.13` maps to `typesafe/jev-1.13`; `jev-latest` maps to `~typesafe/jev-latest`. The documentation warns that the TypeSafe SDK's model-list parser is incompatible with OpenRouter's listing response. [Official integration documentation](https://openrouter.ai/docs/guides/community/typesafe-sdk).

Read-only verification on the research date:

- The [versioned model page](https://openrouter.ai/typesafe/jev-1.13) lists the model and the same advertised input price.
- The public [provider endpoint metadata](https://openrouter.ai/api/v1/models/typesafe/jev-1.13/endpoints) returned `text->decisions`, a 32,000-token context, TypeSafe as provider, and prompt pricing `0.000000042` dollars per token with zero completion pricing.
- A general Models API response contained no matching Jev entry during this observation. Absence from that particular listing therefore did not mean the dedicated integration was unavailable.
- The [official SDK implementation](https://github.com/OpenRouterTeam/go-sdk/blob/main/systemone.go) exposes a System One operation and automatic retry handling. SDK retries must be governed by Sigy's attempt and reservation rules.

These observations establish a documented integration candidate, not a validated billing adapter. Do not transfer first-party context limits, model identifiers, retention policy, cancellation guarantees, or ordinary chat parameters to this endpoint. Pin a supported version, retain the resolved model identity, and revalidate aliases before use. SDK availability does not select Go or any other application language.

The integration documentation's example `usage.cost` is not a measured invoice reconciliation. Pricing metadata, billable-token semantics, retries, fees, and authoritative request lookup must agree before strict paid admission can be enabled. Do not assume the general generation-lookup or provider-routing controls support this new endpoint identically.

## Spending proposal

Jev is optional and disabled until the user configures a finite paid classification allowance. It uses the existing [cost policy](../docs/planning/07-providers-and-cost-policy.md), including global, provider, monitor, stage, and per-attempt limits. A cheap individual call can still create substantial recurring spend across many streams.

Illustrative arithmetic only: 10,000 classified blocks at 2,000 total billable input tokens each produce 20 million tokens. At the advertised $0.042 per million, their token charge would be **$0.84**. This excludes any other billable dimensions, retries, transcription, translation, synthesis, identification, taxes, and account fees. It is not a quoted daily cost or a reservation bound. Repeating full context across separate calls increases billable work.

Proposed controls:

1. Filter deterministically and reuse compatible transcript/translation derivatives before sending text.
2. Cache by input revision, model identity, complete question definitions, taxonomy, and processing policy. Never reuse a changed question's old answer.
3. Batch independent questions sharing one bounded state when the adapter contract permits. Keep outputs associated with exact passages; avoid one huge ambiguous bundle.
4. Reserve a verified maximum before every potentially billable submission. Disable opaque SDK retries or admit each attempt explicitly.
5. Stop the paid stage at its cap. Continue authorized capture and local work within their own limits. Do not silently fall back to another paid model.
6. Show actual, reserved, and unresolved spend, along with blocks analyzed, abstained, queued, and skipped.

If input-token accounting or another charge cannot be conservatively bounded, keep that route unavailable in strict mode until the contract is resolved.

## Local alternatives worth comparing

| Candidate | Evidence and potential | Open qualification work |
| --- | --- | --- |
| Rules and lexicons | Transparent, inexpensive baseline for known names and categories | Synonyms, morphology, code switching, spelling/ASR errors, recall on new events |
| Embeddings with a small supervised classifier | Reuse local semantic features and learn a narrow task from reviewed labels | Labeled data, drift, class imbalance, calibration, embedding/runtime footprint |
| GLiClass Multilang Edge | Official card describes an approximately 140M-parameter multilingual zero/few-shot classifier, 20 training languages, and Apache 2.0 licensing | Native runtime/export, tokenizer and postprocessing parity, radio-domain quality, unsupported languages, small-host latency |
| Multilingual mDeBERTa NLI classifier | Established zero-shot classification baseline; card lists MIT licensing and multilingual pretraining/fine-tuning | Per-language quality differs from pretraining coverage; hypothesis wording, label-count cost, conversion/quantization parity |
| Small local generative model with constrained labels | Can reuse a configured text model | Higher generation cost, format validation, inconsistent labels, calibration and latency |

Sources: [GLiClass model card](https://huggingface.co/knowledgator/gliclass-multilang-edge), [mDeBERTa model card](https://huggingface.co/MoritzLaurer/mDeBERTa-v3-base-mnli-xnli). Vendor GPU throughput and broad language claims do not establish CPU capacity or regional coverage in Sigy. These candidates are alternatives, not implementations of Jev's exact probability contract.

Native inference is plausible through facilities such as the [ONNX Runtime C API](https://onnxruntime.ai/docs/get-started/with-c.html), but that does not prove a particular model exports correctly. Python examples in model cards do not authorize a Python application or runtime. Review weights, tokenizer, runtime, conversion artifacts, and notices separately. No weights have been downloaded or inspected here.

## Evaluation and decision gates

Use a majority non-English, licensed reference corpus with station/time splits that prevent repeated syndicated content leaking between calibration and evaluation. Preserve the provenance of existing human annotations when present; no new reviewer recruitment is required by the current plan. Include noisy ASR, mixed languages, unknown topics, negation, satire, quoted allegations, music announcements, and adversarial instructions. Compare:

- Original-language classification versus classification of English translations, recording translation errors, extra delay, and total cost.
- Rules, each viable local classifier, Jev, and the proposed synthesis model on the same examples.
- End-to-end relevant-event recall, precision, false negatives, abstention, per-language calibration, latency, resource use, and cost per useful supported finding.
- The full cascade against analysis without a classifier. High classification accuracy alone does not demonstrate useful savings.

Set acceptable missed-event rates and per-language thresholds before evaluation. Retain an authorized audit sample of low-scoring passages and an exploration allowance to detect filtering blind spots. Classification does not delete recordings or convert unprocessed material into evidence of absence.

Model changes invalidate old threshold qualification. Changed taxonomies need explicit versions and either reprocessing of comparable historical windows or visible discontinuity in trend charts. Local processing remains the default whether or not Jev qualifies.

## Near future

Recheck this newly introduced API, billing semantics, pinned model availability, language behavior, and local classifier runtimes at the integration gate. Open-source local decision models may become stronger candidates; the stable boundary is a typed, versioned decision result, not dependence on one vendor's terminology.
