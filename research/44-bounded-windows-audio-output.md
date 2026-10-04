# Bounded Windows audio output

Review date: 2026-10-04. This records source inspection and the selected implementation contract. Device output, acoustic delivery, native fault qualification and integrated verification require separate dated experiment evidence.

## Why a separate sink

A bounded local probe of the installed Gyan FFmpeg `9.0.1-full_build-www.gyan.dev`, built with GCC 16.1.0, found only `caca` among output devices. `dshow`, `gdigrab`, `lavfi`, `libcdio`, `openal` and `vfwcap` were inputs; neither `wasapi` nor `dsound` was available. The five-second probe inspected bounded redirected output and opened no audio device. Its GPL, version3 and SDL2 build flags do not establish an audio output muxer. These observations describe that installation, not all Windows builds. Existing null-output playback proves decoding and reader lifecycle rather than sound.

Keep FFmpeg as the protected-stream decoder and use a narrowly scoped Rust output sink. The selected dependency is Windows-only CPAL 0.18.2 with default features disabled. Its released API exposes device configurations, callbacks, initialization timeout and stream timestamps. Streams begin paused. Neither an API success nor a playback timestamp establishes that someone heard sound. [Released CPAL documentation](https://docs.rs/cpal/0.18.2/cpal/).

## Lifetime and authority

CPAL belongs inside an internal helper child of the same application binary. The released WASAPI destructor joins a worker without a timeout, and native event waiting can block indefinitely. Play and pause queue commands; native discovery can also block outside the stream initialization timeout. An abandoned blocking task cannot provide closure evidence. This source behavior motivates process containment rather than an in-process deadline claim. [Released WASAPI stream](https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/wasapi/stream.rs), [device implementation](https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/wasapi/device.rs).

The parent reuses the existing native supervisor and processkit 3.3.4 group, with decoder and helper in one bounded client operation. Raw Windows spawning creates a suspended process, assigns the job and resumes it. Inspection found that this raw path does not preserve arbitrary command creation flags. Abrupt owner death before assignment can leave an unassigned suspended child; creation-time containment remains an explicit limitation. Assignment can fail under nested-job or security restrictions, and Windows does not retrospectively account for memory allocated before assignment. Fail closed rather than launch an uncontained fallback. [ProcessKit 3.3.4 Windows source](https://github.com/ZelAnton/ProcessKit-rs/blob/v3.3.4/src/sys/windows.rs), [Microsoft assignment restrictions](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject).

The helper receives duration bounds and PCM, not a library path, URL, recording identity or capture authority. Default output is selected once and resolved to a fixed endpoint. No automatic device reroute occurs. Original-file protection ends only after the service's actual encoded reader closes. Decoder completion, sink completion and an observed empty native group are separate facts. A forced close cannot be reported as drained presentation.

## Selected output contract

Require the actual default F32 profile at 8 to 192 kHz and one or two channels. Unsupported profiles receive fixed, redacted refusal codes. Negotiate before configuring FFmpeg resampling and channel conversion. Input frames have a four-byte little-endian length, at most 32,768 payload bytes and complete interleaved channel frames. An explicit zero-length End followed by EOF is required. Configuration is bounded to 4 KiB; at most two strict JSON events are emitted, each bounded to 8 KiB.

The safe single-producer/single-consumer atomic ring holds at most 250 ms and 1 MiB. Start after 50 ms of pre-roll or the complete shorter interval. Callback work allocates nothing, blocks on nothing and logs nothing. Nonfinite samples fail. Finite overshoot is saturated only at device output to `[-1,1]`, with exact accepted `clipped_samples`; originals remain unchanged and full-queue retries cannot inflate the counter. Underrun and drain zero fills are separately counted rather than described as source silence.

PCM lifetime bytes use checked arithmetic and the declared 100 ms decoder tolerance. Cooperative work checks cover content and End; the parent process deadline also covers blocked reads, discovery and Drop. Final content consumption alone is insufficient: the callback must publish its predicted presentation time before drain can succeed. That estimate remains explicitly unproven acoustic delivery.

Released FFmpeg defines `N/A` progress when no output timestamp exists. Bounded initial unknown observations must retain that meaning, followed by a measured final time; they cannot establish progress or justify accepting a negative timestamp. The endpoint validation records the first failure and subsequent parser repair separately. [FFmpeg 9.0.1 progress implementation](https://github.com/FFmpeg/FFmpeg/blob/n9.0.1/fftools/ffmpeg.c).

## Dependency and distribution review

Selective resolution adds 29 package/version pairs while preserving existing versions and checksums. The normal Windows target traversal selects only two new packages: CPAL, Apache-2.0, and dasp_sample 0.11.0, MIT OR Apache-2.0. Existing Windows bindings are reused. The other 27 additions are inactive target closures, including ALSA, Apple and Android dependencies; retention in the lockfile does not qualify their compilation or runtime. All 29 normalized manifest license declarations were inspected. Preserve applicable license files, source notices and NOTICE obligations, including the existing Unicode-3.0 notice. [Pinned CPAL manifest](https://github.com/RustAudio/cpal/blob/v0.18.2/Cargo.toml), [CPAL license](https://github.com/RustAudio/cpal/blob/v0.18.2/LICENSE).

No new SDL DLL or ASIO SDK is selected. Optional ASIO, JACK, realtime and browser features remain disabled; qualify with `CPAL_ASIO_DIR` absent. Exact lock advisories and binary distribution obligations remain separate gates. The installed GPL-enabled FFmpeg cannot be described as LGPL-only. [CPAL build script](https://github.com/RustAudio/cpal/blob/v0.18.2/build.rs), [FFmpeg legal guidance](https://ffmpeg.org/legal.html).

## Alternatives and evidence needed

FFplay uses FFmpeg and SDL, but adds another executable, queues and coordinated ownership. SDL wrappers add native distribution work. Direct WASAPI or custom FFmpeg builds expand native maintenance. The selected slice keeps one application language and introduces no Python. [FFplay documentation](https://ffmpeg.org/ffplay.html).

Independent PCM, protocol, queue, deadline and hostile-input fixtures precede controlled output trials. Then measure known content, seek latency, underruns, resource use, interruption and actual group closure. Explicit device tests and optional consented loopback must remain distinguishable from acoustic acceptance. Neither synthetic fixtures nor silent decoding establish audible support.
