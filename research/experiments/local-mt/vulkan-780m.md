# Vulkan translation on the Radeon 780M

Run: 2026-09-24, 14:12 to 14:34 (-07:00), on the development host (Windows 11 Pro 10.0.26200, Ryzen 7 7840U with 8 cores and 16 threads, about 64 GiB RAM, integrated Radeon 780M on AMD driver 32.0.31007.5012). Scope: throughput, load time, device evidence and output equality of the llama.cpp CPU and Vulkan builds for the two local translation models of the [32-clip calibration](calibration-32-translation.md), and an assessment of a Vulkan build of the whisper.cpp recognizer. This is measurement evidence for a research harness. It is not a capacity claim, a quality claim, or a qualification of the [local translation worker](../../../docs/decisions/0041-local-translation-worker.md).

## Assets

| Object | Identity |
| --- | --- |
| CPU runtime | llama.cpp `b11146` (commit `7fe450e19305b828c199d602c23a8337aaa1f03b`), `llama-b11146-bin-win-cpu-x64.zip`, as recorded in the [32-clip calibration](calibration-32-translation.md) |
| Vulkan runtime | Same release, `llama-b11146-bin-win-vulkan-x64.zip`, 32,127,004 bytes, SHA-256 `55a378aa095b466979d85075234f66d7655c7a7483222af0c006c0e55b4d7bd6`, equal to the GitHub release API `digest`; `ggml-vulkan.dll` SHA-256 `7a5c5ec81b01678ca2fc8203e9f37b02a80e55299cff06c629e5d39b6d1e4778` |
| Model A | `Hy-MT2-1.8B-Q4_K_M.gguf`, SHA-256 `dc5f44fcf1fa496ee7ad725982c0c8c553a4de00259b53af84c4b89fb0c06699` (re-hashed for this run) |
| Model B | `gemma-4-E2B-it-Q4_0.gguf`, SHA-256 `8e30dff3ac4c8434c49a7036fa15564bdbb6044e42bf04550bf1a096ad7e6a52` (re-hashed for this run) |
| Vulkan stack | Driver loader `vulkan-1.dll` 1.4.341.0; `vulkaninfo --summary`: `AMD Radeon(TM) 780M`, integrated GPU, AMD proprietary driver, `driverInfo = 26.5.2 (LLPC)`, API 1.4.344, conformance 1.4.3.3 |

The Vulkan package is the CPU package plus one file. Every other file in the two archives is byte-identical, including `llama-completion.exe` (SHA-256 `3427f711f8d20ddb4f141cd89f3ef0c4351e389fa5d54af1d503ff778560518a`), `llama-bench.exe` (`8a465234f89ae29d9a157a2fd8793e012989ec78d3ee21613bc7fcf6126af7c3`), `ggml-base.dll` (`be315e18c795d15658f5d13b4a6e0b4cd534b7d69f6d4093db60ea74b0ada159`) and `ggml-cpu-zen4.dll` (`06660a9ef42529bd49ee9f0fbc517a966c8b188097157c0eff0bbda256bdf75b`). Both builds use dynamically loaded backends, so the only difference is whether `ggml-vulkan.dll` is present in the directory.

The only transfers were the Vulkan archive, release metadata from the GitHub API, and pinned whisper.cpp and llama.cpp source files for the assessment below. No driver, SDK or system software was installed, and the user's Ollama (running, idle at about 47 MiB) was not touched.

## Devices

`llama-completion --list-devices` with the Vulkan package:

```
Available devices:
  Vulkan0: AMD Radeon(TM) 780M (48956 MiB, 46508 MiB free)
```

With the CPU package it reports `(none)`. `llama-bench` also prints the backend line:

```
ggml_vulkan: 0 = AMD Radeon(TM) 780M (AMD proprietary driver) | uma: 1 | fp16: 1 | bf16: 1 | fp4: 0 | warp size: 64 | shared memory: 32768 | int dot: 1 | matrix cores: KHR_coopmat
```

`uma: 1` confirms unified memory. The 48,956 MiB the driver reports is shared system memory, not dedicated video memory.

## Method

Every process ran alone, in sequence, with `OMP_WAIT_POLICY=PASSIVE`, standard input from the null device and a wall deadline enforced with `taskkill /F /T` (none was reached). Configurations were interleaved round-robin (CPU 4 threads, CPU 8 threads, Vulkan) so that shifting host load spread across them.

| Configuration | Runtime | Flags |
| --- | --- | --- |
| `cpu-t4` | CPU package | `-t 4 -ngl 0` |
| `cpu-t8` | CPU package | `-t 8 -ngl 0` |
| `vk` | Vulkan package | `-t 4 -ngl 99 -dev Vulkan0` (all layers) |

- **Throughput:** `llama-bench -p 256 -n 64 -r 3 -o json`, run as 3 separate processes per model and configuration, giving 9 samples per metric. Flash attention was left at `auto`.
- **Translation runs:** `llama-completion` with the calibration harness settings (`--offline --jinja -st --no-display-prompt --no-warmup -n 256 -c 2048 --temp 0 -s 0 -lv 4`, prompt in a file with `-f`). Hy-MT2 translated 3 FLEURS calibration sentences 3 times per configuration (9 runs each); Gemma 4 E2B translated the Arabic sentence 3 times per configuration with the raw-prompt route of the calibration record. `-lv 4` is required: at the default verbosity `llama-completion` prints none of the offload lines.
- **Load time** is the log timestamp of the `system_info` line, reached after backend initialization, model load, device upload and context creation. The `load time` value from `common_perf_print` measures something narrower and is not used.
- **Host load:** `Win32_Processor.LoadPercentage` was sampled before each process. It read 57 to 100%, mostly 80 to 100%, with a VMware virtual machine (about 32 GiB working set), Python processes and several coding sessions running. Free memory stayed between about 20.8 and 25.5 GiB.

Sentences (FLEURS train, calibration partition): `ar_eg` 16932136382444340406 (sentence 1087), `hi_in` 9207457166478158214 (1087) and `es_419` 8957261904601052060 (264).

## Results

Throughput from `llama-bench` (median of 9 samples, range in parentheses). Prompt processing is a 256-token batch; generation is 64 tokens.

| Model | Backend | Threads / layers | Prompt t/s | Generation t/s |
| --- | --- | --- | ---: | ---: |
| Hy-MT2 1.8B Q4_K_M | CPU | 4 / 0 | 108.0 (102.9 to 120.7) | 27.4 (17.7 to 33.0) |
| Hy-MT2 1.8B Q4_K_M | CPU | 8 / 0 | 161.0 (132.2 to 187.9) | 23.2 (9.1 to 33.9) |
| Hy-MT2 1.8B Q4_K_M | Vulkan 780M | 4 / 33 of 33 | 748.5 (662.9 to 891.4) | 42.5 (40.9 to 45.4) |
| Gemma 4 E2B Q4_0 | CPU | 4 / 0 | 80.1 (52.3 to 89.5) | 5.6 (3.2 to 21.9) |
| Gemma 4 E2B Q4_0 | CPU | 8 / 0 | 53.2 (18.1 to 111.8) | 5.4 (0.9 to 11.7) |
| Gemma 4 E2B Q4_0 | Vulkan 780M | 4 / 36 of 36 | 632.1 (566.3 to 677.4) | 27.0 (24.0 to 29.8) |

Translation runs from `llama-completion` (median, range in parentheses; Hy-MT2 n = 9, Gemma n = 3). Prompts were 77 to 114 tokens after templating, so these rates are lower than the batch rates above.

| Model | Backend | Threads / layers | Load s | Prompt t/s | Generation t/s | Wall s |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| Hy-MT2 | CPU | 4 / 0 | 1.67 (1.26 to 2.89) | 98.5 (22.3 to 123.6) | 25.4 (1.7 to 36.6) | 4.45 (3.31 to 27.80) |
| Hy-MT2 | CPU | 8 / 0 | 1.49 (1.29 to 2.25) | 133.3 (23.9 to 159.8) | 19.6 (3.6 to 33.9) | 4.58 (3.37 to 14.12) |
| Hy-MT2 | Vulkan 780M | 4 / 33 of 33 | 1.73 (1.28 to 2.47) | 355.7 (257.4 to 474.6) | 43.7 (37.2 to 45.4) | 3.28 (2.64 to 4.43) |
| Gemma 4 E2B | CPU | 4 / 0 | 3.00 (2.90 to 4.28) | 72.8 (62.7 to 87.2) | 18.8 (16.3 to 25.0) | 6.25 (5.47 to 8.10) |
| Gemma 4 E2B | CPU | 8 / 0 | 3.08 (2.60 to 3.34) | 86.4 (78.6 to 117.2) | 15.0 (9.2 to 21.7) | 6.64 (5.21 to 8.44) |
| Gemma 4 E2B | Vulkan 780M | 4 / 36 of 36 | 4.59 (3.30 to 9.66) | 217.0 (5.9 to 275.5) | 22.9 (9.3 to 27.8) | 7.60 (5.71 to 27.60) |

Observations:

- Vulkan was faster and far steadier. Prompt processing was about 5 to 12 times the CPU rate in `llama-bench` and 2.5 to 3.6 times on the short translation prompts. Generation was about 1.5 times the 4-thread CPU rate for Hy-MT2 and about 5 times for Gemma in `llama-bench`, and 1.2 to 1.7 times on the translation runs. The CPU generation ranges are wide because the host was saturated; the Vulkan ranges stayed narrow under the same load.
- Eight CPU threads helped prompt processing for Hy-MT2 but did not help generation, and for Gemma both were worse and more variable than 4 threads on this loaded host.
- Load time was not reduced by Vulkan. Uploading weights and creating the device context cost about as much as the CPU load, and the Gemma Vulkan load was longer. With one process per cue, load time dominates short cues on every backend.
- **First-use shader cost.** The first Vulkan run of each model paid a one-time cost: Hy-MT2's first probe spent 3.2 s evaluating a 34-token prompt (later runs about 0.16 s), and Gemma's first translation spent 13.1 s on a 77-token prompt, making it 27.6 s wall against 5.7 to 7.6 s afterwards. Later processes were fast, which is consistent with a driver-side pipeline cache that persists across processes. The cache was not cleared or inspected, so the cold cost after a driver update or cache eviction is not bounded by this run.

## Device evidence

Hy-MT2, Vulkan (from a translation run log):

```
llama_prepare_model_devices: using device Vulkan0 (AMD Radeon(TM) 780M) (unknown id) - 46508 MiB free
load_tensors: offloading output layer to GPU
load_tensors: offloading 31 repeating layers to GPU
load_tensors: offloaded 33/33 layers to GPU
load_tensors:      Vulkan0 model buffer size =  1075.74 MiB
load_tensors:  Vulkan_Host model buffer size =   193.57 MiB
llama_kv_cache:    Vulkan0 KV buffer size =   128.00 MiB
sched_reserve:    Vulkan0 compute buffer size =   243.97 MiB
common_memory_breakdown_print: |   - Vulkan0 (Radeon(TM) 780M) | 48956 = 46508 + (1447 =  1075 +     128 +     243) +        1000 |
```

Gemma 4 E2B, Vulkan:

```
llama_prepare_model_devices: using device Vulkan0 (AMD Radeon(TM) 780M) (unknown id) - 46508 MiB free
load_tensors: offloaded 36/36 layers to GPU
load_tensors:      Vulkan0 model buffer size =  1434.76 MiB
load_tensors:  Vulkan_Host model buffer size =  1668.00 MiB
sched_reserve:    Vulkan0 compute buffer size =   535.50 MiB
```

Gemma keeps 1,668 MiB of weights in host-visible memory even with every layer offloaded. CPU runs log `offloaded 0/33 layers to GPU` and `CPU_REPACK model buffer size = 711.00 MiB`. `llama-bench` JSON reports `backends: Vulkan`, `gpu_info: AMD Radeon(TM) 780M` and `n_gpu_layers: 99` for the Vulkan rows and `backends: CPU` for the CPU rows.

**Silent offload.** The Vulkan package run with exactly the product worker's current arguments (no `-ngl` and no `-dev`, `-n 512 -c 4096`), a cleared environment containing only `PATH` (runtime directory and `System32`), `SystemRoot` and `OMP_WAIT_POLICY`, logged `using device Vulkan0` and `offloaded 33/33 layers to GPU`. A runtime directory that contains `ggml-vulkan.dll` therefore uses the GPU by default, and the worker would not know because it discards standard error. The same cleared environment with `-ngl 0 -dev none` logged `offloaded 0/33 layers to GPU`. The Vulkan loader worked with that cleared environment.

## Output equality

Greedy settings were identical across backends. Every configuration was deterministic across its own repetitions.

| Model | Sentence | CPU 4 = CPU 8 | CPU = Vulkan |
| --- | --- | --- | --- |
| Hy-MT2 | `es_419` 264 | yes | yes, byte-identical |
| Hy-MT2 | `hi_in` 1087 | yes | yes, byte-identical |
| Hy-MT2 | `ar_eg` 1087 | yes | **no** |
| Gemma 4 E2B | `ar_eg` 1087 | yes | yes, byte-identical |

Hy-MT2 on the Arabic sentence:

- CPU (both thread counts, and equal to the output in the 32-clip record): `The tiger belongs to the same group as the black panther, the rosette panther, and the yiguur. These four are the only ones that can fart.`
- Vulkan, all layers: `The tiger belongs to the same group as the black panther, the spotted panther, and the hyena. These four are the only ones that can bark.`

Isolation runs on the same sentence, one process each:

| Variant | Output ends with | Equal to |
| --- | --- | --- |
| Vulkan package, `-ngl 0 -dev none` | `rosette panther, and the yiguur ... can fart.` | CPU |
| Vulkan, all layers, `-fa on` | `spotted panther, and the hyena ... can bark.` | Vulkan default |
| Vulkan, all layers, `-fa off` | `rosette panther, and the yiguur ... can bark.` | neither |
| Vulkan, 16 of 33 layers | `spotted panther, and the hyena ... can exhale a fart.` | neither |

The binaries are identical, so the difference comes from GPU arithmetic, flash attention and the layer split, not from the build. Greedy decoding flips at a near-tie token, and after a flip the text diverges. Both outputs are wrong translations of "roar"; this confirms that backend and layer split are part of a translation's identity, not that either backend is better. The `hi_in` output on both backends kept the calibration record's meaning error (`There are only four species of lions that can roar.`).

## whisper.cpp with Vulkan

**Release assets.** On 2026-09-24 the newest whisper.cpp release is `v1.9.4`, published 2026-09-11, at commit `927cfce34f31707e17f2bff35c349632fb9e2c3a`, the same commit as the pinned `b5130`. No newer tag exists. The Windows x64 assets of `b5130` are `whisper-bin-x64.zip` (CPU), `whisper-blas-bin-x64.zip` and two NVIDIA CUDA packages; the 15 most recent releases have no Vulkan asset for any platform, and the pinned release workflow has no Vulkan job.

**Building from the pinned source** (`ggml/src/ggml-vulkan/CMakeLists.txt` at `927cfce`) requires:

- CMake 3.19 or newer for the Vulkan backend.
- `find_package(Vulkan COMPONENTS glslc REQUIRED)`: the Vulkan headers, the loader import library and the `glslc` shader compiler, normally from the LunarG Vulkan SDK (`VULKAN_SDK` is added to the CMake prefix path when set).
- `find_package(SPIRV-Headers CONFIG REQUIRED)`: the SPIR-V headers CMake package, also shipped with the SDK.
- A C and C++ compiler for the library and for `vulkan-shaders-gen`, which is built for the host through `ExternalProject_Add` and compiles every shader with `glslc` during the build.
- The README's commands: `cmake -B build -DGGML_VULKAN=1` then `cmake --build build -j --config Release`. For reference, llama.cpp's own Windows Vulkan release job installs Vulkan SDK 1.4.357.0 with the LunarG installer and builds with Ninja and MSVC.

**On this host (checked without installing):** CMake 4.3.2 (w64devkit) and the CMake bundled with Visual Studio Build Tools 2022 17.14, MSVC 14.44.35207, MSBuild, Windows SDK 10.0.26100.0, GCC 16.1.0 and Ninja are present. The Vulkan runtime loader and `vulkaninfo` come with the driver. The Vulkan SDK is absent: no `VULKAN_SDK`, no `C:\VulkanSDK`, no `glslc`, no `glslangValidator` and no SPIRV-Headers package. A Vulkan build of whisper.cpp therefore needs a user-approved SDK installation first; it was not attempted.

**Reusing llama.cpp's `ggml-vulkan.dll`.** The `b5130` x64 build uses dynamically loaded backends (`-DGGML_BACKEND_DL=ON`), so `whisper-cli` would try to load any `ggml-*.dll` placed beside it. A static comparison, with nothing executed:

- whisper `b5130` carries ggml 0.23.0 (synced from ggml `e91ded11bdcd78c42f9c8d3978ff6686eb4c1226`); llama.cpp `b11146` carries ggml 0.25.1.
- `GGML_BACKEND_API_VERSION` is 2 in both, and `ggml-backend.h`, `ggml-backend-impl.h` and `ggml-vulkan.h` are identical.
- `ggml.h` differs in 73 lines: new precision values (`GGML_PREC_BF16`, `F16`, `Q8`, `Q4`) and functions (`ggml_prec_set_acc`, `ggml_prec_set_src`), deprecations and new operator constructors.
- All 35 symbols that `ggml-vulkan.dll` imports from `ggml-base.dll` are exported by whisper's `ggml-base.dll`.

The DLL would therefore probably load, and nothing would detect a semantic mismatch between a 0.25.1 backend and a 0.23.0 core. Neither project publishes or tests that combination, and it would give a recognition profile a runtime assembled from two releases. It is not recommended and was not tried. The supported routes are an upstream Windows Vulkan asset when one appears, or a build from the pinned commit with an approved Vulkan SDK, as a new recognition engine template; the current `whisper-cpp-cli-v1` template passes `--no-gpu`.

## Limitations

- One host, one driver, one session of about 22 minutes on a heavily shared machine. CPU figures in particular depend on the concurrent load; none of these numbers is a latency or capacity claim.
- The 780M shares system memory and memory bandwidth with the CPU. Concurrent GPU translation and CPU recognition or capture were not measured.
- Three sentences for Hy-MT2 and one for Gemma. Equality holds only for these inputs; a larger set will likely show more backend differences.
- Memory under a Job Object was not measured. The recognition decision already notes that the committed-memory ceiling excludes GPU memory; whether driver allocations on this unified-memory device count against a job's commit limit is unknown.
- The driver shader cache was neither cleared nor inspected, so the cold first-use cost is an observation, not a bound.
- The Ollama service was running (idle) and its GPU use was not observed.

## Recommendation

Add GPU translation as a separate, explicit profile, never as a mode of the CPU profile:

1. **New engine template**, for example `llama-completion-vulkan-v1`, with its own immutable profile. Its identity already covers every runtime file hash, so the Vulkan package (which adds `ggml-vulkan.dll`) is a different profile from the CPU package. Add the device selector, layer count and flash-attention setting to the identity.
2. **Pinned flags:** `-dev Vulkan0 -ngl 99 -fa on`, plus the existing greedy arguments and `-lv 4` so the evidence lines are printed. `-fa on` produced the same output as `auto` on this device. Also pass `-fit off` so the runtime cannot adjust parameters; that flag was not part of these measurements and needs a check.
3. **Pin the CPU profile too:** the existing worker should pass `-ngl 0 -dev none`, and profile creation should refuse a CPU profile whose runtime directory contains a GPU backend DLL. Today a user who registers the Vulkan package as a CPU profile silently gets GPU inference.
4. **Device evidence:** at profile creation, run `--list-devices` once and store the device name. On each run, read standard error up to a fixed bound, as untrusted text, and require `using device Vulkan0 (<stored name>)` and `offloaded N/N layers to GPU` with both numbers equal. Store the device name, layer count and the `Vulkan0 model buffer size` in the job result as executor evidence. Missing or partial offload, or a different device name, fails that cue with a reason such as `device-unavailable`. It is not retried on the CPU.
5. **Fallback is a separate job:** when a GPU profile fails, the job reports it. Running the CPU profile is a new explicit request or saved policy, producing its own translation revision under the CPU profile. Because the outputs can differ (Arabic above), the stored revision must name which profile produced it, and both profiles need their own calibration before any quality claim.
6. **Deadlines and admission:** the per-cue deadline must allow the observed first-use shader cost (13 s for Gemma), or profile creation should run one warm-up translation. Treat the GPU as one exclusive admission slot with a device-memory budget (about 1.5 GiB for Hy-MT2 and 2 GiB of device memory plus 1.7 GiB of host-visible memory for Gemma at `-c 2048`), separate from CPU threads, and do not rely on the Job Object memory ceiling to bound it.
7. **Next measurements:** a Job Object memory test with Vulkan, a longer calibration comparing CPU and Vulkan outputs and scores, and a persistent-process design, since load time now dominates short cues on both backends.

## Workspace

Scratch files are under `.agents/language-evaluation/gpu/`: the Vulkan archive and extracted runtime, `release-b11146.json` and `whisper-releases.json` (API metadata), `bench.jsonl` (SHA-256 `57f930225a9745c2474f631127f938b8614f2d0fe3dbfa2fffcf79194efe762c`), `complete.jsonl` (`f5bb256646cbcb15d8f1ab733c30312a9acd9139f731f3ccbe64bea842828ea2`), per-process logs in `logs/`, the harness `measure.py` (`74b2a7cbf12096d39639dc440ede2aa5c98660e76748b94653ef595f0e48b8c2`), `analyze.py` and the fetched source files in `src/`.
