# Language and stack trade study

Status: historical pre-selection comparison. Research baseline: 2026-09-20. The subsequent [foundation decision](../decisions/0001-rust-foundation.md) selects Rust for initial development and records the incomplete qualification matrix. See [primary-source language research](../../research/07-language-and-stack.md).

## 1. What the decision must optimize

The goal is the strongest maintainable product over years of operation. Development convenience is relevant only as part of lifecycle cost, reviewability, and defect risk. More implementation difficulty is justified when it buys measurable reliability, performance, or maintainability.

No language is universally best for every version of this product. The critical question is whether Sigy mainly coordinates external media/model workers or also owns substantial native streaming and signal-processing paths. The confirmed hardware roadmap means this must be examined now.

## 2. Hard requirements before scoring

- Native support for the chosen Windows, macOS, and Linux release matrix.
- A high-quality terminal ecosystem that passes the proposed interaction tests.
- Reliable long-running services, explicit shutdown, worker supervision, and durable storage integration.
- Bounded concurrency and memory ownership at media/hardware boundaries.
- Model-provider integration independent of convenience SDKs.
- Adequate tools for testing, profiling, fuzzing, dependency management, and reproducible release procedures.
- A maintainable native-library strategy and sustainable cross-platform packaging.
- Correct multilingual rendering and extensible typed signal/cryptographic boundaries without requiring one language for every existing native dependency.
- No Python application/runtime dependency in the chosen product stack.

A candidate that fails a hard requirement cannot compensate through a high average score elsewhere.

## 3. Preliminary comparison

The assessments below are engineering interpretations of documented language/runtime properties, not benchmark results.

| Dimension | Rust | Go | What decides it for Sigy |
| --- | --- | --- | --- |
| Memory/resource behavior | Ownership and deterministic destruction offer explicit resource-lifetime control; native/unsafe code remains a boundary | Managed memory and concurrent GC simplify many structures; native allocations and soft heap limits need separate accounting | Whole-process memory, buffer reuse, overload behavior, and reviewability |
| Native media and radio integration | Direct FFI can fit explicit buffers/callbacks; safety contracts need careful wrappers | cgo works but adds calling, pointer, and cross-build constraints; external workers can reduce this surface | Actual sample rates, call frequency, copy budget, cancellation, packaging |
| Service coordination | Strong type modeling and async tooling; cancellation/runtime choices require care | Goroutines and networking tooling are well suited to coordination; cancellation and leak prevention remain explicit work | Implementing and debugging supervision, backpressure, recovery, and client events |
| TUI | Ratatui/Crossterm are research candidates with direct rendering control | Bubble Tea is a research candidate with message-driven application structure | Focus, Unicode, layout, input, accessibility, latency, maintainable state |
| Deployment | Native binaries; external libraries and target toolchains affect packaging | Straightforward pure-Go distribution; cgo changes the build story | Fully assembled installer, dependencies, signing, updates, supported architectures |
| Maintenance | Ownership/type contracts can make invalid states harder to express; async/generic complexity needs restraint | Compatibility commitment and a compact language can support straightforward service maintenance | Readability, debugging, dependency churn, contributor competence, upgrade effort |
| High-rate samples | Strong reason to investigate direct buffer handling without GC in that path | Viable with well-designed boundaries; keeping sample processing in native workers changes the comparison | Physical-device or representative replay measurements |
| Model throughput | Mostly determined by selected model/runtime when inference is external | Same | Compare orchestration overhead separately from inference |

Rust is a particularly important candidate for a resource-conscious core that may grow deeper native signal handling. Go remains a serious candidate for an architecture whose heavy data paths stay inside well-isolated native workers. These are hypotheses to test, not a selection.

At this historical checkpoint, the engineering recommendation was to evaluate a **Rust-first application profile**: a small Cargo workspace, terminal candidates, clap for command parsing, an embedded SQLite catalog, and typed adapters to maintained native media/model workers. The subsequent [foundation decision](../decisions/0001-rust-foundation.md) selected Rust 1.98.1 and SQLite; the [terminal decision](../decisions/0015-terminal-stack.md) selected Ratatui with Termina on the measured Windows host. Exact current dependencies are in `Cargo.lock`. These selections do not prove release reliability or other platforms. [Repository engineering](12-repository-and-engineering.md) tracks remaining packaging and dependency qualification.

## 4. Proposed evaluation weights

| Criterion | Weight | Evidence |
| --- | --- | --- |
| Correctness and recoverability | 25% | Lifecycle, concurrency, fault-injection, and invariant results |
| Long-term maintainability | 20% | Design review, debugging exercises, dependency and upgrade review |
| Resource efficiency and predictability | 20% | CPU, memory, latency, process count, soak and overload results |
| Native media/hardware fit | 15% | Buffer/callback design, sample throughput, cancellation, build results |
| CLI/TUI quality | 10% | User journeys, terminal matrix, automation contract |
| Packaging and operational support | 10% | Install/update/recovery on supported hosts |

Weights are proposed and should be settled before scoring. There are no fabricated numeric candidate scores in this draft.

## 5. Comparable future experiments

After the documentation phase is reviewed, construct the smallest experiments that resolve the actual uncertainty:

1. A persistent service with durable job submission, reconnecting clients, bounded events, and worker cancellation.
2. The same media worker feeding the same archive contract under both candidates, with identical faults.
3. The same transcript fixture rendered with filters, scrolling, search, resizing, and Unicode input.
4. A representative native callback/buffer handoff or IQ replay path, including backpressure and shutdown.
5. The same simultaneous budget-reservation and uncertain-provider-outcome scenarios.
6. Native build/package exercises for both machine classes and all primary OS targets.

Measure performance and record implementation complexity: unsafe/native surface, cancellation states, platform-specific branches, dependency count/health, diagnostic quality, and time to understand a seeded failure. Avoid optimizing one candidate while leaving the other as an unbounded naive implementation.

The terminal comparison includes the same rotatable globe, day/night shading, marker clustering, linked search/list state, DVR timeline, and bounded audio/activity views. Measure this under actual capture/inference contention and slow-terminal output, with accessible fallback modes. A library's flat-map demo alone does not satisfy the requested experience. Verify the full CLI independently of the renderer. [Explorer contract](11-radio-explorer-and-dvr.md).

## 6. Other stack decisions remain independent

| Component | Candidate approaches | Selection condition |
| --- | --- | --- |
| Media | FFmpeg workers/libraries, a GStreamer pipeline, dedicated playback adapter | Timing, isolation, format coverage, distribution evidence |
| Catalog | Embedded transactional database, server database if requirements justify it | Recovery, concurrency, backup, footprint, search results |
| Local control | Protected sockets/named pipes, authenticated loopback protocol | Identity, API evolution, platform behavior, future clients |
| Speech | Native streaming engine, windowed ASR engine, approved remote endpoint | Quality, latency, language coverage, footprint, cost bounds |
| Text models | Ollama, other local/LAN runtimes, OpenRouter and other approved providers | Capability, destination policy, result quality, cancellation/billing |
| Extensions | Versioned worker protocol or narrowly reviewed native adapter | Throughput, isolation, lifecycle, independent upgrades |
| Search | Lexical, semantic, or hybrid retrieval | Evidence recall and latency by language, index and compute cost |
| Signal transforms | Native DSP/decoder workers, existing supported headless interfaces, narrowly reviewed embedded libraries | Typed compatibility, time mapping, sample throughput, isolation, maintenance and license fit |
| Cryptography | Maintained native libraries or qualified language-native implementations | Operation/profile coverage, independent review, key handling, conformance and platform support |

Do not assume a single binary means a dependency-free installation. Codecs, drivers, certificates, model assets, and accelerator libraries still need an operational plan. Do not introduce an application-language hybrid unless a specific measured benefit outweighs extra interfaces and release work.

Future web or native clients do not require choosing their framework now. Keep the service interface and domain model independent so those clients can be evaluated when their requirements are known.

## 7. Selection record and remaining qualification

The [foundation decision](../decisions/0001-rust-foundation.md) names the selected compiler, initial packages, measured local probes, alternatives and reconsideration conditions. Subsystem, platform, model and release qualifications still need their own measured records.

This trade study was written during documentation-first planning and creates no code or runtime evidence by itself. Implementation and bounded experiments are now active under the [build order](../../ROADMAP.md#build-order).
