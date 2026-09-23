# Speech recognition and translation

Reviewed: 2026-09-20. Status: candidate research; no model selection, download, or inference benchmark. The [2026-09-22 active evaluation plan](../docs/development/language-pipeline.md) supersedes the review method below where it differs and does not depend on newly arranged human reviewers.

## Findings

whisper.cpp provides local speech inference and a live-input example. Upstream describes that example as a basic approach that repeatedly processes incoming audio. A demonstration of live input does not establish production-grade incremental captions or the required latency. [Upstream documentation](https://github.com/ggml-org/whisper.cpp#real-time-audio-input-example).

sherpa-onnx documents local speech recognition, streaming and non-streaming model paths, and builds for Linux, macOS, Windows, and embedded systems. This makes it a relevant alternative for a persistent, lower-latency pipeline. Availability of a runtime on an OS does not prove every model or accelerator works there. [Official documentation](https://k2-fsa.github.io/sherpa/onnx/index.html).

Ollama provides text generation and structured outputs, useful for translation, extraction, and planning. That does not make any configured model an audio recognizer. Capabilities must be tested per runtime and model. [Generation API](https://docs.ollama.com/api/generate), [structured outputs](https://docs.ollama.com/capabilities/structured-outputs).

## Architecture alternatives

| Approach | Strength | Limitation to test |
| --- | --- | --- |
| Streaming ASR followed by translation | Incremental text and explicit stage timing | Model/language coverage, partial revisions, translation context |
| Windowed multilingual ASR followed by translation | Broad candidate ecosystem and common batch/live processing | Overlap handling, repeated text, context/latency tradeoff |
| Direct speech translation | Potentially fewer stages | Original transcript availability, alignment, language-pair coverage, provider dependence |
| Hosted speech endpoint | May offload local compute | Network dependency, audio destination, billing bounds, cancel semantics |

The leading proposal keeps original-language ASR and translation separately inspectable. Direct speech translation remains a candidate profile if it satisfies evidence and timing requirements.

## Quality design

Maintain separate provisional and finalized utterances. Preserve the original language, timestamps, model/configuration identity, and uncertainty indicators. Corrections create new revisions and invalidate dependent translations or findings where appropriate.

Speech activity detection must be evaluated on radio music beds, noise, ads, silence, call-ins, and overlapping speech. Disabling analysis of non-speech does not discard original audio. Language detection supports a manual override and explicit uncertainty on short clips.

Translation needs bounded context, terminology preferences, named-entity preservation, and resistance to instructions embedded in the transcript. Literal quotation and summarized interpretation are separate output types.

For constrained hosts, a lower-capacity live profile and a queued higher-quality pass may coexist. Label the profile used. A job must not silently reduce its declared quality or switch destinations under load.

## Evaluation plan

Build a consented or appropriately licensed evaluation set representative of intended radio use. Most expected listening is non-English; a majority of evaluation speech duration must be non-English with meaningful per-language minimums. French-to-English is one journey, not the boundary of language support. Qualify actual launch language/task profiles using the broader [multilingual plan](10-multilingual-processing.md). Do not assume a model's advertised language list is a quality guarantee.

Measure:

- Word/character error rates by language and acoustic condition, including names and numbers.
- False speech on silence or music, omitted speech, code-switching errors, and repeated text.
- Partial-caption delay, finalized utterance delay, translation delay, and revision rate.
- Meaning preservation, omissions, unsupported additions, terminology, and readability against licensed published references, with calibrated independent model checks where useful.
- Time alignment sufficient to replay the cited passage.
- Real-time factor, throughput at several concurrent sources, RAM, accelerator memory, and thermal stability.
- Behavior when a model is unavailable, a worker is cancelled, or processing falls behind.

Publish results per profile and corpus, not a single average across languages. Reference-based and calibrated model checks have limits; a model grading its own output is insufficient, and no result is described as human-reviewed unless that review occurred.

## Near future

The expanded [local-processing and capacity study](17-local-processing-and-capacity.md) compares current recognizer families, language-routing alternatives, no-metered-fee operation, durable live/batch queues, and the arithmetic needed to qualify multiple simultaneous streams. It includes Omnilingual ASR, Qwen3-ASR, Parakeet, and the separate coverage/licensing limits of language detectors without selecting a runtime.

Watch native streaming multilingual models, local accelerator support, and efficient translation models. Keep model/runtime adapters replaceable and evaluate upgrades on frozen test subsets. Do not select the application language based on the training language of a model or a convenience SDK.
