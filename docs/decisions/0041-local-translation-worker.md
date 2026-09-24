# Local translation worker

Date: 2026-09-24. Status: implemented and tested on Windows x86_64 (catalog and local IPC v29). This is roadmap operation 25's storage and worker. Translation quality is not qualified by this record.

## Decision

`analysis translate` translates one recognized transcript revision into English with a pinned local profile, supervised by the service. The portable baseline is a llama.cpp `llama-completion` CPU runtime with a pinned GGUF model; the first template is `hy-mt2-plain-v1` for Tencent Hy-MT2. A user-run Ollama or a GPU backend can become optional profiles later; none is assumed.

## Profiles

`analysis translation-profile add` hashes a local runtime directory and model into an immutable profile, like a [recognition profile](0039-native-recognition-worker.md). The identity covers the engine, template, file hashes and sizes, the declared source languages, threads, the committed-memory ceiling and the per-cue deadline, not file locations. Declared languages are a canonical, sorted list of primary subtags. They are a declaration from the model publisher, not measured quality. Profiles cannot be changed or deleted, and they are not exposed through `sigy mcp`.

## Execution

Admission accepts only a recognition transcript with text. The worker receives the cue texts and the recognizer's block language label, never media, a source URL, a catalog handle or a secret. It re-hashes the profile files first; any change fails the job as `profile-unavailable` before a process starts.

If the block label is `en`, every cue is untranslated with reason `source-english`. If the label names a language the profile does not declare, every cue is untranslated with `unsupported-language`. Without a label, translation is attempted.

Each cue runs in its own contained process group (one process, the profile's memory ceiling, a CPU rate equal to its thread count, kill-on-close, the per-cue deadline, cancellation) with `--offline`, greedy decoding, at most 512 generated tokens, a cleared environment and null standard input. One process per cue guarantees that an English cue maps to exactly one source cue; no word alignment is invented. The prompt is written to a private scratch file. Standard output is read up to 64 KiB. Terminal escape sequences, the end-of-text marker and surrounding whitespace are removed as formatting; nothing else is changed. A cue that exceeds its deadline, fails, overruns the output bound, or returns empty, NUL-bearing, oversized or non-UTF-8 text is stored as untranslated with that reason rather than failing the whole job.

Worker processes run with `OMP_WAIT_POLICY=PASSIVE`. On this host under load, spinning OpenMP threads inside a CPU-rate-limited job measured 21 seconds per generated token against 1.2 seconds with passive waiting. The same setting applies to the recognizer.

## Storage

A completed job publishes one immutable translation revision of that exact transcript revision, with one row per source cue (`translated` with English text, or `untranslated` with a reason) and a zero-USD amount, in one transaction with the terminal job state. SQL triggers require the running job, its generation and profile, the next revision number and exactly one row per source cue. Cancellation wins over a queued result. Restart marks running jobs interrupted with a new generation, so a late result is refused as stale. Replay of a job ID returns history; a changed request under that ID is refused. Translations and their cues cannot be updated or deleted.

`analysis translation` shows the original script and the English text side by side with media times and reasons, labeled as unreviewed machine translation.

## Evidence

Storage tests cover exact replay and one active worker, zero cost, immutability, misaligned, hostile and oversized results, reasons, cancellation, restart and stale results, and refusal of transcripts without recognized text. The native-media fixture drives the real CLI and service with a fault-injecting stand-in translator: translated pairs on the media clock, exact and conflicting replay, an undeclared language, a failing exit, an output flood, a deadline, and a changed model file. The stand-in also confirms that the environment is cleared and passive waiting is set.

A manual run with llama.cpp b11146 and Hy-MT2 1.8B Q4_K_M translated Canadian French broadcast cues correctly where time allowed, and left Swahili untranslated as an undeclared language. The same model made a critical meaning error on an Arabic calibration sentence in an earlier check. No language pair is validated.

## Limitations

- One process per cue reloads the model each time; long transcripts are slow.
- The block language label applies to the whole transcript; mixed-language recordings are translated as one language.
- A translator can follow instructions embedded in speech; the output is labeled as machine text and grants nothing.
- The model can hallucinate or change meaning. Validation against published references with critical-error checks is required before any quality claim.
- No operating-system network sandbox; `--offline` and a cleared environment are the only network controls.
