# Audio, capture, playback, and timing

Reviewed: 2026-09-20. Status: documented capabilities; no backend selected or benchmarked.

## Findings

FFmpeg documents network protocol restrictions, I/O timeouts, HTTP behavior, and stream metadata. These controls are relevant to managing unreliable remote streams and limiting what nested inputs can access. [Protocol documentation](https://ffmpeg.org/ffmpeg-protocols.html).

Its upstream muxer documentation describes segmenting media. A segmenter is a useful primitive, but the application still has to validate media, track committed objects, preserve time relationships, and recover interrupted files. [Muxer documentation source](https://github.com/FFmpeg/FFmpeg/blob/master/doc/muxers.texi).

GStreamer provides composable media pipelines and a clock/timestamp model for synchronization. It is a credible alternative when the application needs continuously reconfigured pipelines, several consumers, and detailed media timing. [Pipeline overview](https://gstreamer.freedesktop.org/documentation/application-development/introduction/gstreamer.html), [synchronization design](https://gstreamer.freedesktop.org/documentation/additional/design/synchronisation.html).

mpv supports control through a client API or JSON IPC, including Unix sockets and Windows named pipes. Its documentation explicitly says the IPC protocol is not a secure network interface. A Sigy adapter would expose a constrained application API and keep player control private. [mpv manual](https://mpv.io/manual/stable/#json-ipc).

FFmpeg's distribution obligations depend on build configuration and enabled components. Bundling, linking, and version updates need a reviewed distribution plan that retains required legal notices. [License and distribution guidance](https://ffmpeg.org/legal.html).

## Options

| Option | Potential strengths | Questions to resolve |
| --- | --- | --- |
| Supervised media executables | Fault isolation; independent versioning; readily testable failure boundaries | Process startup, controlled IPC, buffering, metadata fidelity, cross-platform packaging |
| Embedded media libraries | Direct sample access and integrated control | Native crash boundary, memory ownership, ABI/version compatibility, build complexity |
| Managed pipeline engine | Explicit clocks, branching, dynamic elements | Deployment size, plugin inventory, error propagation, operational debugging |

Do not use the same integration strategy automatically for playback, internet recording, and high-rate IQ. Their data rates and isolation requirements differ.

## Proposed media contract

Capture must work without an audio-output device. Speaker playback is a separate consumer of captured or buffered audio, with its own volume and device state. Listening and recording must not create subtly different timelines that make captions refer to the wrong audio.

Archive original compressed content where reliable. Normalized PCM is a derivative for analysis. Evaluate lossless segments where compressed-source recovery is too fragile. Frame/sample offsets remain authoritative for navigation; wall-clock mappings include source delays and gaps.

Use bounded buffers, explicit reconnect deadlines, and process supervision. Reconnect must not concatenate unrelated time intervals into an apparently continuous recording. Handle a codec or sample-rate change as an explicit media configuration event.

Pre-roll and rewind require a rolling buffer whose duration and storage budget are visible. A bookmark can pin a bounded interval without changing the retention of an entire day.

## Capacity calculations

The following are arithmetic estimates in decimal units, before filesystem, container, metadata, or redundancy overhead:

| Representation | Approximate data rate | One hour |
| --- | --- | --- |
| 128 kbit/s compressed audio | 16 kB/s | 57.6 MB |
| 16 kHz, 16-bit, mono PCM | 32 kB/s | 115.2 MB |
| 48 kHz, 16-bit, stereo PCM | 192 kB/s | 691.2 MB |
| 20 million complex samples/s, 8-bit I plus 8-bit Q | 40 MB/s | 144 GB |

Eight 128 kbit/s streams require about 11.06 GB/day of compressed payload. Decoder, inference, and indexing costs are additional. Continuous IQ has a very different storage profile from internet radio; it requires a separate admission profile.

## Required experiments

Compare backend options using identical fixtures: direct MP3, AAC, HLS, changed codec, stalled connection, redirects, truncated frames, reconnect gaps, and long recordings. Measure sustained bytes, sample continuity, seek accuracy, process count, resident memory, CPU, shutdown, and recovery.

Inject process termination during segment finalization and catalog commit. Test a busy/missing audio device and output hotplug while captures continue. Validate Windows, macOS, and Linux packages independently.

## Near future

Prefer mature decoding paths with observable state. Replace a backend only after repeating the same capture-integrity and timing suite. Model changes should not require changes to the archive format.
