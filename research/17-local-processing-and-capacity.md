# Local multilingual processing and capacity

Reviewed: 2026-09-20. Status: research and proposed capacity model. No model execution, download, throughput benchmark, or hardware qualification.

## Product requirement

Local language detection, speech recognition, and queued processing should make sustained multi-station monitoring useful without metered inference charges. Both live and batch workflows matter. A local profile must remain useful when no paid provider is configured.

Here, **$0 local processing means no per-request model-service fee**. Hardware, electricity, storage, bandwidth, and obtaining permitted music reference data still have costs. It does not mean unlimited real-time inference on every machine. A user's paid API budget can be zero while locally authorized capture and analysis continue.

Excellent speech recognition for every language, dialect, acoustic condition, and singing style is not an established capability. Advertised coverage is a candidate list. Sigy needs qualified language/task profiles, experimental support, explicit unsupported/unknown states, and replaceable recognizers.

## Current local candidates

| Candidate | Documented capability | Important boundary |
| --- | --- | --- |
| whisper.cpp | Native local multilingual recognition and multiple acceleration paths | Its basic live-input example is not a production streaming guarantee; evaluate language-specific quality, overlap, and hallucinations |
| sherpa-onnx | Native speech runtime with streaming and non-streaming model families and cross-platform targets | Select and qualify an exact model/runtime/backend combination |
| Omnilingual ASR | Upstream documents recognition across 1,600+ languages, with CTC and LLM model families | Broad advertised transcription coverage does not establish equivalent language detection, translation, or radio quality |
| Qwen3-ASR family | Official card describes language identification and ASR for 30 languages plus 22 Chinese dialects, with offline and streaming inference | Dialects are not 52 independent languages; timestamps and runtime paths have their own coverage and cost |
| Parakeet TDT 0.6B v3 | Official card describes automatic language detection and recognition across 25 European languages | Not a worldwide-language recognizer; non-European language coverage needs other models |

Sources: [whisper.cpp](https://github.com/ggml-org/whisper.cpp), [sherpa-onnx](https://k2-fsa.github.io/sherpa/onnx/index.html), [Qwen3-ASR model card](https://huggingface.co/Qwen/Qwen3-ASR-1.7B), [Parakeet model card](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3).

Omnilingual ASR is particularly relevant to the desired worldwide coverage. Upstream's December 2025 update lists improved v2 CTC/LLM checkpoints and a separate unlimited-length LLM variant. Results from the largest model must not be assigned to smaller variants or earlier conversions. Source language/script identifiers also need explicit mapping to Sigy's language metadata. [Official repository](https://github.com/facebookresearch/omnilingual-asr). The publisher describes model/code assets as Apache 2.0 and the released dataset as separately CC BY licensed. [Publisher announcement](https://ai.meta.com/blog/omnilingual-asr-advancing-automatic-speech-recognition).

sherpa-onnx documents 300M and 1B Omnilingual CTC models, including quantized exports, with [C/C++ API examples](https://k2-fsa.github.io/sherpa/onnx/omnilingual-asr/c.html). Its illustrated 300M package is dated November 12, 2025, before the upstream v2 update. The example transcription leaves the language field empty. Do not claim that this native path supplies the latest model, calibrated language detection, or the large model's accuracy. [Supported models and example output](https://k2-fsa.github.io/sherpa/onnx/omnilingual-asr/models.html). These are concrete integration candidates, with conversion provenance and per-language radio evaluation still required.

The [sherpa-onnx Qwen3-ASR documentation](https://k2-fsa.github.io/sherpa/onnx/qwen3-asr/index.html) lists a 0.6B integration and exported model paths. That is stronger integration evidence than assuming an arbitrary model can run through a generic endpoint. Verify conversion provenance, quantization quality, packaging, and genuine incremental behavior rather than equating VAD-triggered chunk recognition with native streaming. Parakeet's card lists CC BY 4.0 for its weights; required legal attribution must be preserved if used. Neither candidate is selected.

Published throughput often uses large accelerators, high batch sizes, or different audio conditions. It does not predict a small always-on host. A native Sigy application also does not require the model author's training framework at runtime; establish a supported native execution path before accepting a candidate.

## Proposed language and processing flow

1. Capture and commit the authorized source independently of inference.
2. Decode a reusable analysis representation and detect speech/music/mixed/unknown intervals. Preserve source audio even if a detector excludes an interval from ASR.
3. Estimate language on sufficient speech, using station metadata only as a prior. Keep multiple candidates and uncertainty; re-evaluate on changes and after recognition.
4. Select a permitted local recognizer based on observed language, quality profile, machine capability, and live/batch deadline. A station can need different recognizers over time.
5. Publish provisional and finalized original text with timestamps and revision identity. Queue higher-quality reprocessing when policy permits.
6. Translate supported spans to English or the configured target. Cache compatible work shared by monitors.
7. Classify and extract from the retained text; aggregate and synthesize with evidence and coverage.

A recognizer's built-in language estimate may avoid a separate detection pass. Compare that approach with an independent detector and with targeted verification of uncertain spans. Do not make a low-confidence detector permanently route a language to the wrong model. Unknown material waits, uses a permitted broad profile, or receives a scoped user override; uncertainty must not trigger an unapproved paid fallback.

An independent broad-coverage reference is MMS LID 4017: its card describes a one-billion-parameter audio classifier over 4,017 language classes, but lists **CC BY-NC 4.0** licensing. It is not a default unrestricted-commercial-use dependency for an Apache-licensed product. Evaluate licensing and permitted deployment separately; a large class list is also not proof of calibrated performance on short radio passages. [Official model card](https://huggingface.co/facebook/mms-lid-4017).

Detection evaluation needs top candidates, unknown/out-of-distribution rejection, related-language confusion, minimum usable speech duration, and code-switch boundaries. A softmax over known labels always chooses something unless the application defines abstention. Keep spoken-language identification separate from optional text-language verification, since an incorrect transcript can make the latter confidently wrong. African and other regional language coverage must be tested explicitly rather than inferred from success on European news.

Speech transcripts, sung lyrics, track titles, and station metadata have separate language fields. Ordinary ASR can be unreliable on singing and cannot establish a recording's identity. Music-only intervals should not consume the full speech pipeline by default, but speech over music and DJ announcements need explicit evaluation.

## Live and batch scheduling

Maintain durable queues measured in **audio seconds, predicted work, retained bytes, and oldest age**, not just item count. Each item names its input interval/revision, stage, model profile, priority, deadline, attempts, and retention dependency.

Live captions have a latency target. Background monitors have freshness targets. Batch reprocessing has completion targets. Use bounded work units, deadline-aware admission, fairness between sources, and an explicit batch allocation or aging policy so continuous live traffic cannot starve queued work forever. Accelerator kernels may not be safely preemptible; bound work before dispatch.

Share model instances and compatible decoded/transcribed work where safe. Bound batch size and wait time, measure memory fragmentation and model-load churn, and prevent a slow stream or large file from blocking unrelated work. Different privacy or quality policies may prevent reuse.

When overloaded, preserve capture within its authorized storage limits, report delayed processing, and apply the saved queue policy. Do not relabel overdue live work as successful live translation. If backlog exceeds limits, pause admission or stop affected work visibly. Queued inputs need retention protection with a finite allowance; neither silent deletion nor unlimited pinning is acceptable.

## Capacity arithmetic to verify later

Define `lambda` as incoming audio seconds requiring analysis per wall-clock second and `mu` as measured processed audio seconds per wall-clock second under the actual workload. A sustained queue needs `mu > lambda` with margin. If backlog is `B` audio seconds, an approximate drain time under unchanged arrivals is `B / (mu - lambda)`; it is undefined as a completion estimate when `mu <= lambda`.

For a single serial worker, real-time factor is processing seconds divided by audio seconds. In a simplified example, eight continuous streams at RTF 0.1 consume 0.8 worker-seconds per second before other overhead. This is arithmetic, not a claim that any candidate achieves that rate. Batching, mixed models, translation, speech occupancy, and hardware contention require measured service curves rather than linear extrapolation.

Storage is a separate limit. Eight 128 kbit/s recordings require approximately 11.06 GB per day in decimal units before container overhead, replicas, or derived audio. Keeping 16 kHz mono 16-bit PCM adds approximately 2.765 GB per stream-day. Bound intermediate caches instead of retaining every decoded representation indefinitely.

Measure capture-only, detection-only, ASR, translation, classification, and complete pipelines separately. Extend the existing small-host and desktop stream-count bands only when earlier bands remain stable. Include all-speech worst cases, majority non-English mixed content, network reconnects, model cold starts, simultaneous playback, disk pressure, and thermal soak.

## Music at scale

Local metadata parsing, interval detection, compatible fingerprint extraction, deduplication, and statistics can avoid per-request processing fees. Identifying arbitrary music also requires a reference catalog with suitable rights and regional coverage. A fingerprint algorithm alone is not a worldwide music database. Free remote services are still remote and have usage limits and terms.

Once a track is identified, classifiers can help organize supported genres, themes, or editorial categories. They must not infer exact song identity, artist nationality, sung language, or popularity from an unverified title. Compute weekly counts and rates from deduplicated play events with explicit monitored-hour and unknown-airtime denominators. The full post-release plan remains in [Monitoring and music](09-monitoring-and-music.md).

## Qualification outcome

Publish a capability profile with capture count, live ASR count, live translation count, sustainable batch throughput, backlog limits, qualified languages, power/memory measurements, and model/runtime hashes. A small host can be valuable as a recorder with delayed local analysis even when it cannot translate every captured station live. Selection follows evidence, without changing the language/stack decision process.
