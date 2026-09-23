# First local ASR asset proposal

Reviewed: 2026-09-22. Planning only. No model, audio, runtime archive, or GPU SDK was downloaded, and no inference was run. No candidate is qualified. `download-manifest.json` is a proposed contract, not an implemented downloader.

## Candidate and exact objects

Start with multilingual `small` and the plain Windows x64 CPU package. The [current stable release v1.9.4](https://github.com/ggml-org/whisper.cpp/releases/tag/v1.9.4) has no attached assets and explicitly points to [build b5130](https://github.com/ggml-org/whisper.cpp/releases/tag/b5130). Both resolve to commit `927cfce34f31707e17f2bff35c349632fb9e2c3a`. The [release API](https://api.github.com/repos/ggml-org/whisper.cpp/releases/tags/b5130) publishes the archive size and SHA-256.

| Object | Exact bytes | Publisher SHA-256 |
| --- | ---: | --- |
| [whisper-bin-x64.zip](https://github.com/ggml-org/whisper.cpp/releases/download/b5130/whisper-bin-x64.zip) | 8,573,270 | `f9ec6c52a2e949b62ab51fa21d0d497958f9e41c3010c157c4e42932d5316f3c` |
| [ggml-small.bin](https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.bin) | 487,601,967 | `1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b` |
| Total | **496,175,237** | About 473.19 MiB |

The model revision is `5359861c739e955e79d9a303bcbc70fb988958b1`; its [Hub metadata](https://huggingface.co/api/models/ggerganov/whisper.cpp?blobs=true) supplies the LFS size and SHA-256. The README's 40-character model checksum is SHA-1, not the SHA-256 above. These are publisher identities, not locally verified asset hashes. The [runtime's model instructions](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/models/README.md) recommend this converted repository and distinguish multilingual `small` from `small.en`.

The [conversion format](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/models/convert-pt-to-ggml.py) embeds model parameters, mel filters, and tokenizer vocabulary in one binary. No separate tokenizer download or Python conversion is needed for this candidate. Avoid VAD, diarization, translation, and extra model downloads in the initial ASR smoke profile.

## Runtime closure and licenses

The [pinned release workflow](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/.github/workflows/release.yml) builds shared libraries and dynamically loaded CPU variants, then archives the Release output directory. Source evidence indicates `whisper-cli.exe`, `whisper.dll`, `ggml.dll`, `ggml-base.dll`, and the applicable `ggml-cpu*.dll` backend files. The archive also packages other examples and SDL2 2.28.5. [CLI linkage](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/cli/CMakeLists.txt) uses `common` and `whisper`, while [SDL linkage](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/CMakeLists.txt) belongs to `common-sdl`.

The archive has not been inspected. Exact entries, expanded sizes, PE imports, CPU backend selection, MSVC/OpenMP runtime requirements, and the minimal executable closure remain an execution gate. Inventory and hash every retained member after validating the outer archive, reject unexpected paths, and inspect imports before running even `--help`. Do not add a redistributable download silently. Use an absolute executable path, a reviewed private runtime directory, and a minimal environment; dynamic backends must not come from a writable working directory or inherited search path. Reuse the already installed decoder for bounded 16 kHz mono PCM16 WAV preparation.

whisper.cpp is [MIT](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/LICENSE). The selected converted [model card declares MIT](https://huggingface.co/ggerganov/whisper.cpp/blob/5359861c739e955e79d9a303bcbc70fb988958b1/README.md), consistent with the original [Whisper code and weights license statement](https://github.com/openai/whisper/blob/86098128c0b4f24f0e2aa2994de830614b474227/README.md#license) and [MIT text](https://github.com/openai/whisper/blob/86098128c0b4f24f0e2aa2994de830614b474227/LICENSE). Preserve those notices and all archive third-party notices. Separately, the current `openai/whisper-small` Transformers [card](https://huggingface.co/openai/whisper-small/blob/973afd24965f72e36ca33b3055d56a652f456b4d/README.md) declares Apache-2.0. That is a different asset route and is not the selected download. Do not describe all Whisper mirrors or distributions as having identical metadata. This plan does not qualify redistribution of the binary package.

## CPU first, conditional Vulkan comparison

There is no Vulkan asset in b5130. The [Vulkan build](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/ggml/src/ggml-vulkan/CMakeLists.txt) requires Vulkan with `glslc` and SPIRV-Headers. Read-only host inspection found CMake, Ninja, `vulkaninfo`, and Vulkan loader 1.4.341.0; `glslc` and `glslangValidator` were not on PATH and `VULKAN_SDK` was unset. This does not prove the SDK is absent everywhere or that the Radeon 780M can execute this backend. No device probe ran.

A later Vulkan build must pin this same source commit and every missing build input, reserve their downloads, and record actual device selection. Reuse the same model and clips. No Vulkan package, SDK, or additional driver is allowed by this manifest. Job commit limits do not establish a bound on GPU/shared-memory use.

For CPU, the [verified CLI flags](https://github.com/ggml-org/whisper.cpp/blob/927cfce34f31707e17f2bff35c349632fb9e2c3a/examples/cli/cli.cpp) support an explicit local model and file, `--no-gpu --threads 2 --processors 1 --language auto --output-json-full`. This overrides the English language default and requests transcription, preserving original-script output. A reference-language-hinted comparison must be separately labeled. Record all decoder defaults; validate actual JSON timestamps and token fields before mapping them to media time. One block language result is not per-span language evidence. Canadian French remains unqualified; Navajo and Klingon remain required gaps.

## Budget and next gate

The two objects consume 496,175,237 bytes from the 10,737,418,240-byte cumulative allowance. Earlier corpus notes record 17,730,888 TSV bytes per complete pass and 260,774 other local-command response bytes, but repeated TSV passes and this research's metadata/document requests are not a reconciled cumulative ledger. Do not treat one pass as the whole prior spend. The manifest therefore keeps acquisition disabled with `prior_download_bytes: null`.

A proposed conservative prior accounting ceiling of 1 GiB plus 1 MiB for future metadata leaves **9,166,452,603 bytes** after these two objects. That is conditional budget arithmetic, not measured remaining allowance or permission to overwrite unknown accounting. Reconcile prior transfers or record an evidenced conservative upper bound before admission. Failed transfers and retries retain their consumed bytes. Reserve each exact object before transfer and update the cumulative receipt during streaming.

A separate 2 GiB local workspace reservation would cover these retained objects, one additional staged model copy, at most 128 MiB of extracted runtime files, and bounded scratch; reconcile it against the existing 20 GiB workspace ceiling. Reject a larger archive instead of silently raising the extraction cap. Count successful/failed setup and cleanup staging separately from inference memory.

Next: reconcile bytes; implement/review the manifest-enforcing acquisition path; inspect the CPU archive and licenses; finish process/network/resource fault proofs; then acquire only the selected model and an explicitly approved, licensed audio manifest. The frozen 112-clip corpus acquisition remains blocked, and this plan contains no substituted audio. Only after those gates should a finite CPU smoke run measure original-script validity, timing, resource peaks, cancellation, and quality. No support or throughput claim follows from an upstream model label.
