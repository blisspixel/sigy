# Retained-recording language evaluation

Reviewed: 2026-09-22. Status: primary-source research and repository inspection. No model inference, corpus/model download, paid request, or device qualification was performed. Runtime choices below are candidates. The [implementation plan](../docs/development/language-pipeline.md) owns sequencing and acceptance; [active work](../docs/development/progress.md) owns status and spending.

## What needs to be decided

Choose a bounded local recognition and English-translation profile for retained audio, preserving original script and exact media provenance. Compare actual language quality and Windows resource behavior before adding production dependencies. A successful API request, model load, or stored empty transcript cannot answer this question.

The first survey is French, Spanish, Portuguese, Arabic, Swahili, Hindi, Mandarin, and English. Canadian French, Navajo, and Klingon remain named required cases, with missing routes or evaluation material visible. The current host is a Ryzen 7 7840U laptop with about 64 GiB RAM, Radeon 780M driver `32.0.31007.5012`, and Ollama `0.34.2`. Those values were inspected locally; acceleration and capacity were not measured.

## Language tag boundary

The storage increment selects `oxilangtag` 0.1.6 for BCP 47 grammar parsing and case normalization. It was released on 2026-05-23, uses MIT licensing, and has no mandatory dependencies; optional serialization is not enabled. The service additionally rejects repeated variants and extension singletons. It does not claim registry validation or deprecated-alias canonicalization. Preserve the raw provider label and mapping version. [Parser API](https://docs.rs/oxilangtag/0.1.6/oxilangtag/struct.LanguageTag.html), [upstream manifest](https://github.com/oxigraph/oxilangtag/blob/v0.1.6/Cargo.toml).

`language-tags` 0.3.2 was considered but its embedded registry data and observed private-use parsing limitation make it a weaker fit for the narrow grammar boundary. A custom parser would duplicate maintained grammar machinery. Neither alternative establishes runtime language quality. The selected crate stays in the service; the domain core remains dependency-free.

## Windows process boundary review

The repository already uses `process-wrap` 10.0.0. Its Windows Job Object path starts a child suspended, assigns the job, then resumes it; the safe public API does not expose job-memory or CPU controls. The root-process `try_wait` result does not establish descendant completion. Reuse the supervisor and wrapper for a model-free worker fixture, but terminate the owned job on cancellation/failure and keep admission occupied until bounded cleanup completes. Cap output bytes before line or UTF-8 parsing. Existing `winsafe` 0.0.29 has no Job Object API. The reviewed `win32job` 2.0.3 working-set control permits paging and does not supply the proposed aggregate committed-memory bound. [Working-set API](https://docs.rs/win32job/2.0.3/win32job/struct.ExtendedLimitInfo.html).

A narrowly reviewed wrapper extension could set typed nonzero `JOB_OBJECT_LIMIT_JOB_MEMORY` limits before assignment/resume and expose accounting. This is a proposal, not an implemented capability or permission to weaken workspace lints. Any retained patch needs upstream provenance, license preservation, and fault tests. Thread flags are cooperative; CPU-rate controls or affinity require separate qualification. Job committed-memory limits do not establish bounds for file-backed mapped assets or GPU/shared allocations. [Job memory fields](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information), [CPU-rate fields](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_cpu_rate_control_information).

Job Objects do not deny network access. An offline Windows Sandbox with fixed memory, read-only input, and bounded output is a candidate CPU evaluation environment, subject to host edition/feature readiness and positive controls for the observer and denial rules. It does not qualify host iGPU performance or contain an existing host Ollama server. AppContainer would require a separate reviewed native boundary. No OS feature or service was changed during this review. [Sandbox configuration](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-configure-using-wsb-file), [requirements](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/).

## Native recognition candidates

| Candidate | Primary-source evidence | Comparison role and limits |
| --- | --- | --- |
| Multilingual Whisper small with whisper.cpp | [v1.9.4 release](https://github.com/ggml-org/whisper.cpp/releases/tag/v1.9.4), [native runtime](https://github.com/ggml-org/whisper.cpp), [model card](https://huggingface.co/openai/whisper-small) | A compact CPU baseline with native Windows and Vulkan paths, avoiding Python execution. Use multilingual weights. Runtime and original weights use MIT licensing. Language accuracy, hallucinations, and timing still need measurement; a block language token does not establish word-level language spans |
| Qwen3-ASR 0.6B int8 with sherpa-onnx | [v1.13.8 release](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v1.13.8), [publisher model](https://huggingface.co/Qwen/Qwen3-ASR-0.6B), [native integration](https://k2-fsa.github.io/sherpa/onnx/qwen3-asr/index.html), [export example](https://k2-fsa.github.io/sherpa/onnx/qwen3-asr/pretrained.html) | Apache 2.0 model and native runtime route. The publisher describes 30 languages and 22 Chinese dialects; Swahili is outside its advertised set. The documented native export example has empty language/timestamp fields; do not copy the publisher's separate aligner or streaming claims onto this export. Freeze original weights, converter, tokenizer, export, and runtime identities |
| Omnilingual CTC 300M int8 with sherpa-onnx | [upstream model family](https://github.com/facebookresearch/omnilingual-asr), [native export](https://k2-fsa.github.io/sherpa/onnx/omnilingual-asr/models.html) | Broader-language challenger with Apache 2.0 code/models. The documented 2025-11-12 export predates v2 and cannot inherit larger-model or v2 quality claims. Token positions and empty language/duration fields are not measured word intervals or language identification |

The proposed first comparison uses the same short retained PCM inputs and bounded worker profile. These are alternatives, not three planned application dependencies. Record native library licenses and notices separately from model licenses. Quantized and converted models need their own provenance and quality results.

whisper.cpp documents CPU and Vulkan backends; its compatibility list is not a measured 780M result. sherpa has a DirectML build option, while ONNX Runtime describes provider-specific constraints. DirectML is a secondary feasibility probe only if the chosen model's operators execute on the intended device with observable fallback. [sherpa build options](https://github.com/k2-fsa/sherpa-onnx/blob/master/CMakeLists.txt), [DirectML requirements](https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html#requirements).

If the initial baseline fails quality, compare a larger recognizer within the remaining asset/resource allocation. Whisper turbo is an ASR alternative; upstream explicitly states it is not trained for translation. English speech translation therefore cannot be assumed from a general family capability. Sigy's initial translation path still preserves an original-script transcript first. [Task guidance](https://github.com/openai/whisper#command-line-usage).

## Text translation candidates

| Candidate | Evidence | Decision implications |
| --- | --- | --- |
| Hy-MT2-1.8B, publisher GGUF | [model card](https://huggingface.co/tencent/Hy-MT2-1.8B), [GGUF assets and native instructions](https://huggingface.co/tencent/Hy-MT2-1.8B-GGUF) | Apache 2.0. Publisher GGUF reduces conversion uncertainty. Compare Q8_0 first, listed at 1.91 GB, then Q4_K_M at 1.13 GB only if useful. Swahili is outside its advertised set. Native llama.cpp feasibility is documented; Ollama template/import behavior still needs testing. Regional and low-resource quality remain unmeasured |
| MADLAD-400-3B with Rust Candle | [publisher-hosted card and native examples](https://huggingface.co/google/madlad400-3b-mt) | Apache 2.0, broader-language challenger. The card links a roughly 1.65 GB quantized conversion and discloses third-party conversion. Verify that lineage and the native dependency graph. Only 204 of the advertised 400-plus languages were evaluated in the cited work; breadth is not qualification |
| An already installed local text model through Ollama | [API chat reference](https://docs.ollama.com/api/chat), [compatibility reference](https://docs.ollama.com/api/openai-compatibility) | Useful baseline without duplicate assets. Pin the actual local digest and template, finite context/output, and execution location. Existing model availability is not evidence of translation adequacy |
| TranslateGemma 4B | [model card](https://huggingface.co/google/translategemma-4b-it) | Secondary research option: 55 advertised languages, task-specific template, Gemma terms and gated access. Do not treat it as Apache 2.0 or accept new account terms implicitly |

NLLB-200-distilled-600M and MMS LID 4017 are research alternatives with CC BY-NC 4.0 terms. Their distribution/use constraints differ from the permissive shortlist. A language identification label is not an ASR or translation capability. [NLLB card](https://huggingface.co/facebook/nllb-200-distilled-600M), [MMS LID card](https://huggingface.co/facebook/mms-lid-4017).

Swahili stays in the confirmed survey. Compare its recognition through a candidate that advertises that task, such as Whisper or Omnilingual, and investigate a broader translator with verified Swahili coverage. Score unsupported routes separately rather than silently excluding Swahili or forcing it through another language. Download only named required assets: the reviewed MADLAD repository totals 15.7 GB and includes an 11.8 GB unquantized file, while its `model-q4k.gguf` is 1.65 GB. A full snapshot would exceed the initial allowance. [MADLAD file inventory](https://huggingface.co/google/madlad400-3b-mt/tree/main).

## Required language cases

The reviewed Whisper language table, Qwen model list, and Omnilingual identifiers did not establish a ready native Navajo or Klingon ASR route. This bounded review does not prove none exists. Generic French support supplies a candidate for Canadian French testing, not regional validation. [Whisper language table](https://github.com/openai/whisper/blob/main/whisper/tokenizer.py), [Omnilingual identifiers](https://github.com/facebookresearch/omnilingual-asr/blob/main/src/omnilingual_asr/models/wav2vec2_llama/lang_ids.py).

OPUS-MT mul-en lists `nav` and `tlh_Latn`, but its published Tatoeba results are very weak: Navajo BLEU 1.3 and chr-F 0.144; Klingon BLEU 0.2 and chr-F 0.084. That is a useful negative comparison, not evidence the requirement is satisfied. Recheck exact checkpoints and original reference data before interpreting those numbers beyond the published experiment. [Model and results](https://huggingface.co/Helsinki-NLP/opus-mt-mul-en).

Preserve `fr-CA`, `nv`, and `tlh` in the evidence matrix. Unknown or unsupported output is correct failure handling but not substantive processing. Do not fill a corpus with invented references, memorized test answers, or copied franchise dialogue. Continue lawful corpus and model feasibility research without depending on newly recruited human reviewers.

Language identifiers also need a semantic boundary: RFC 5646 distinguishes language tagging for unwritten audio from writing-system choices. Retain written-script evidence on transcript representations and avoid inferring it from a station location or an audio-only language label. [RFC 5646 section 4.1](https://www.rfc-editor.org/rfc/rfc5646.html#section-4.1).

## Portability and this iGPU

Current Ollama documentation says Vulkan is enabled by default when the backend is installed on Windows/Linux. Radeon 780M is absent from its documented Windows ROCm/HIP7 list. Vulkan is the first candidate to compare against CPU here; the same documentation acknowledges unstable integrated-GPU configurations and provides device-selection controls. This is a feasibility inference, not a compatibility claim. [Hardware support](https://docs.ollama.com/gpu).

The reviewed Ollama API documentation covers text/chat, images, and embeddings; no generic speech-transcription endpoint was established by this review. A compatible chat API does not make a model accept audio, supply timestamps, or recognize a language. Ollama's local server can also access cloud models when signed in, so localhost alone cannot qualify a local-only route. [API index](https://docs.ollama.com/llms.txt), [local and cloud endpoints](https://docs.ollama.com/api/introduction).

Use a small task capability contract shared by native, Ollama, and later OpenRouter adapters. Record supported input/output types, language/task coverage, result granularity, model identity, finite limits, execution destination, and billing evidence. Keep hardware acceleration behind the local profile. Do not create a generic provider framework that assumes every endpoint supports the same modalities or accounting.

## Evaluation material

| Source | Reviewed access and license | Appropriate use and limitation |
| --- | --- | --- |
| FLEURS | [official dataset card](https://huggingface.co/datasets/google/fleurs), CC BY 4.0 | Reproducible multilingual audio/transcript baseline. Read speech does not establish broadcast performance. Shared parallel sentence identities must not leak across calibration and holdout |
| FLORES+ | [official dataset card](https://huggingface.co/datasets/openlanguagedata/flores_plus), gated CC BY-SA 4.0, evaluation-only use | Text translation reference material if access terms are accepted separately. Much material is translated from English, which limits natural source-language claims. Do not commit raw gated examples or assume anonymous download |
| CoVoST 2 | [official project](https://github.com/facebookresearch/covost), data described as CC0 with separate code/addendum terms | Source speech and English reference translations. Requires Common Voice v4 audio, whose exact lawful availability must be checked. Modern Common Voice is not a drop-in replacement |
| Common Voice | [current portal](https://mozilladatacollective.com/datasets), [consumer terms](https://mozilladatacollective.com/terms/consumers) | Additional speakers and conditions where permitted. Account, version, and per-dataset terms require review; preserve pseudonymous speaker grouping |
| MUCS SLR104 | [OpenSLR record](https://www.openslr.org/104/), CC BY-SA 4.0 | Natural Hindi-English and Bengali-English code switching. The Hindi-English test archive is listed at 443 MB. Tutorial speech is not radio; sentence timing is not a language-switch annotation |
| WMT24++ | [official card](https://huggingface.co/datasets/google/wmt24pp), Apache 2.0 metadata | English-to-other-language text references include `fr_CA`. Reversing pairs does not qualify Canadian French speech or natural Canadian French-to-English processing |

No verified licensed Navajo/Klingon speech-and-reference package was established in this search. Keep those acquisition gaps explicit. Public availability does not itself permit redistribution or remote model upload. The download manifest must record licenses, source revision, hashes, allowed local/remote use, grouping, exclusions, and reference provenance.

## Automated quality evidence

The user requested evaluation without relying on newly arranged human reviewers. Use human-authored published references where lawfully available, deterministic scoring, and calibrated independent models. Report these methods separately. Reference gaps cannot be repaired by asking the tested model to generate its own correct answer.

NIST SCTK provides primary scoring conventions. SacreBLEU documents reproducible metric signatures and tokenization, but it is Python software and is not approved as a new runtime in this project. A Rust implementation must pass independent known-answer fixtures and preserve the normalization signature. [SCTK](https://github.com/usnistgov/SCTK), [SacreBLEU](https://github.com/mjpost/sacrebleu).

Model-based translation evaluation has published evidence on particular language pairs and tasks; it does not imply dependable judgment of every Sigy span. Judge studies identify position, verbosity, and self-preference biases, and automatic error labels can disagree with human annotations. These findings motivate calibration and explicit uncertainty. [GEMBA](https://aclanthology.org/2023.eamt-1.19/), [judge bias research](https://arxiv.org/abs/2306.05685), [MQM-APE](https://aclanthology.org/2025.coling-main.374/).

Use a fixed error taxonomy for additions, omissions, entities, quantities, negation, attribution, and meaning. Validate judge sensitivity and false positives with published references, controlled critical errors, and meaning-preserving alternatives. Blind candidate identity, swap answer order on a bounded subset, compare different model families, and record failures and disagreement. A judge gets quoted source text and no tool authority. Agreement and back-translation support investigation but do not become ground truth. The [plan](../docs/development/language-pipeline.md#corpus-and-model-comparison) specifies holdout and evidence levels.

## Native resource and recovery evidence

Windows job objects can manage ordinary child trees and kill owned processes when the relevant handle closes. They also have breakaway and security limitations. Microsoft separately documents enforceable committed-memory and CPU-rate controls. The presence of Sigy's existing process wrapper does not prove those controls are configured, nor does it provide network isolation. [Job objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), [memory limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information), [CPU control](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_cpu_rate_control_information).

The implementation therefore needs actual process-tree timeout/cancel, memory/output pressure, missing-model, and capture-contention tests. Record observed peaks separately from enforced limits. GPU shared memory, decoder scratch, mapped weights, and child processes belong in the resource profile. Keep network observation, denied egress, and the absence of application-level paid dispatch as three different evidence claims.

### Windows boundary candidate, 2026-09-22

ProcessKit 3.3.4 is a candidate for the shared process boundary. Its tagged source exposes safe memory, process-count, and CPU-quota options and configures a Windows job before resuming the child. The implementation uses job-wide committed-memory limits, a CPU hard cap, kill-on-close, and group statistics. The manifest declares MIT and Rust 1.88. This is source review, not local qualification or dependency selection. [Manifest](https://github.com/ZelAnton/ProcessKit-rs/blob/v3.3.4/Cargo.toml), [group API](https://github.com/ZelAnton/ProcessKit-rs/blob/v3.3.4/src/group.rs), [Windows implementation](https://github.com/ZelAnton/ProcessKit-rs/blob/v3.3.4/src/sys/windows.rs).

Material limitations remain. The library documents a gap between suspended process creation and job assignment during which abrupt owner death can leave an inert orphan. Its raw group spawn path overwrites creation flags, including the flag that hides a Windows helper window; integration must preserve that behavior through an appropriate safe API. Job termination must be followed by bounded confirmation of zero active members and root reaping before releasing capacity or input leases. Job commit limits do not bound all mapped or GPU/shared memory, CPU quota needs measured interpretation, and the wrapper supplies no network sandbox. Windows Sandbox executables were absent from the inspected host's standard locations; no OS feature was enabled. [Scope and caveats](https://github.com/ZelAnton/ProcessKit-rs/blob/v3.3.4/docs/untrusted-children.md), [upstream group tests](https://github.com/ZelAnton/ProcessKit-rs/blob/v3.3.4/tests/integration/groups.rs), [limit tests](https://github.com/ZelAnton/ProcessKit-rs/blob/v3.3.4/tests/integration/limits.rs).

The next experiment is an isolated Rust fixture harness after dependency, license, and advisory review. Use small bounded child allocations to test aggregate commit refusal, measured CPU saturation, finite process count, invalid-limit rejection, and fail-closed spawn setup. Exercise a grandchild that holds a pipe after root exit, cancellation, deadline, asynchronous abort, owner death, nested jobs, and output without a newline. Preserve the pre-assignment orphan gap in the result unless a maintained safe creation API closes it. Resolve and positively test native network denial separately before running model assets. Start with CPU evidence; the iGPU remains a separate qualification.

## Bounded OpenRouter comparison

The user authorizes up to USD 20 cumulative external spend, including every comparator and judge request, with exact reservations and no automatic limit increase. No amount is reserved by this research. The product's default zero budget remains unchanged; operations 26 and 27 precede live paid testing.

OpenRouter's dedicated transcription documentation states that routing preferences `order`, `only`, and `ignore` do not apply to transcription requests. Output timing varies by model/provider and billing can be duration- or token-based. A policy field ignored by that endpoint cannot enforce destination or cost policy. Keep direct remote ASR unqualified until the actual reachable route and all charges can be bounded. [Speech-to-text documentation](https://openrouter.ai/docs/guides/overview/multimodal/stt.md). This Markdown source was retrieved anonymously when the rendered-page browser failed.

Start the paid comparison with bounded text translation and judge requests through a qualified chat route. Full provider endpoint tags may matter because a base provider name can cover multiple regions/variants. Disable implicit retries, model routers, tools, and fallback initially. Parameter enforcement and effective returned provider identity need fixtures and live readback. `max_price` is a unit-price restriction, not a total-spend cap. [Provider routing](https://openrouter.ai/docs/guides/routing/provider-selection).

The anonymous model and endpoint APIs were inspected for comparison leads, including `google/gemini-2.5-flash-lite` and `google/gemini-2.5-flash`, with separate-family judge leads. No model is selected and catalog prices are not admission snapshots. The two Gemini models are from the same family and do not supply independent judging. Re-fetch exact model/provider metadata and price units at admission. [Model API](https://openrouter.ai/api/v1/models), [example endpoint metadata](https://openrouter.ai/api/v1/models/google/gemini-2.5-flash-lite/endpoints).

Important accounting checks:

- Provider per-token rates may be smaller than Sigy's USD micro-unit. Parse exact decimal/rational rates, multiply by proven billable-unit bounds, then round the total liability upward to integer micro-USD. Do not parse a tiny rate directly as `Usd` or use floating point.
- API rates can be per token while routing `max_price` uses per-million-token units. Record and test the conversion explicitly.
- Output limits may include reasoning only on some providers. The generic reasoning documentation does not establish a universal hard billing bound. Check exact endpoint semantics, hidden reasoning, rounding, request fees, and the complete permitted route set before reserving. [Reasoning guidance](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens.md).
- Audio-chat support has its own accepted formats and metering. A text route's bound cannot be reused for audio without evidence. [Audio inputs](https://openrouter.ai/docs/guides/overview/multimodal/audio).
- Remote use stays remote even with zero-data-retention settings. Dataset terms, intended destinations, and effective endpoint policy must allow the actual submitted excerpts. [ZDR documentation](https://openrouter.ai/docs/guides/privacy/zdr.md).

The first proposed batch is at most USD 2 within the USD 20 ceiling, with sequential calls and no retries. A timeout retains its full reservation until reconciled. Stop on unexpected routing, usage beyond the bound, missing enforcement, or ambiguous accounting. The existing exact runtime ledger and [work ledger](../docs/development/progress.md#spending-ledger) must agree; a separate script cannot create another USD 20 allowance.

## Decision still open

Freeze a small licensed corpus and native asset manifests, build the bounded worker and measurement harness, then compare CPU and iGPU profiles. Select by per-language reference scores, critical semantic errors, usable coverage, resource behavior, and fault recovery. Keep no-network local evidence independent from remote quality comparison. No candidate wins by upstream popularity, advertised language count, fluency, low expected price, or a passing schema test.
